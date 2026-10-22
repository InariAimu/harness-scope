use super::*;
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    thread,
};

fn settings(mode: Mode, proxy_url: &str) -> Settings {
    Settings {
        mode,
        proxy_url: proxy_url.into(),
    }
}

fn listener() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}

fn accept(listener: TcpListener) -> TcpStream {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "No connection arrived"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("{error}"),
        }
    }
}

fn headers(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 8192);
    }
    String::from_utf8(bytes).unwrap()
}

fn http_server(listener: TcpListener, status: &str) -> thread::JoinHandle<String> {
    let status = status.to_owned();
    thread::spawn(move || {
        let mut stream = accept(listener);
        let request = headers(&mut stream);
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
        )
        .unwrap();
        request
    })
}

#[test]
fn manual_proxy_receives_http_and_https_connect_and_does_not_fallback_on_failure() {
    for (url, first_line) in [
        (
            "http://network-fixture.invalid/catalog",
            "GET http://network-fixture.invalid/catalog HTTP/1.1",
        ),
        (
            "https://network-fixture.invalid/catalog",
            "CONNECT network-fixture.invalid:443 HTTP/1.1",
        ),
    ] {
        let proxy = listener();
        let config = settings(
            Mode::Manual,
            &format!("http://{}", proxy.local_addr().unwrap()),
        );
        let server = http_server(proxy, "502 Bad Gateway");
        let response = config.client().unwrap().get(url).send();
        if url.starts_with("https") {
            assert!(response.is_err());
        } else {
            assert_eq!(response.unwrap().status(), 502);
        }
        assert!(server.join().unwrap().starts_with(first_line));
    }
}

#[test]
fn socks5h_proxy_resolves_the_destination_and_carries_the_request() {
    let proxy = listener();
    let config = settings(
        Mode::Manual,
        &format!("socks5h://{}", proxy.local_addr().unwrap()),
    );
    let server = thread::spawn(move || {
        let mut stream = accept(proxy);
        let mut greeting = [0; 2];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting[0], 5);
        let mut methods = vec![0; greeting[1] as usize];
        stream.read_exact(&mut methods).unwrap();
        assert!(methods.contains(&0));
        stream.write_all(&[5, 0]).unwrap();
        let mut request = [0; 5];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request[..4], &[5, 1, 0, 3]);
        let mut host = vec![0; request[4] as usize];
        stream.read_exact(&mut host).unwrap();
        assert_eq!(host, b"network-fixture.invalid");
        let mut port = [0; 2];
        stream.read_exact(&mut port).unwrap();
        assert_eq!(u16::from_be_bytes(port), 80);
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
            .unwrap();
        let request = headers(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
        request
    });
    assert_eq!(
        config
            .client()
            .unwrap()
            .get("http://network-fixture.invalid/catalog")
            .send()
            .unwrap()
            .text()
            .unwrap(),
        "ok"
    );
    assert!(server.join().unwrap().starts_with("GET /catalog HTTP/1.1"));
}

#[test]
fn loopback_bypasses_manual_proxy_and_direct_ignores_proxy_configuration() {
    for mode in [Mode::Manual, Mode::Direct] {
        let proxy = listener();
        let config = settings(mode, &format!("http://{}", proxy.local_addr().unwrap()));
        let origin = listener();
        let url = format!("http://{}/local", origin.local_addr().unwrap());
        let server = http_server(origin, "200 OK");
        assert_eq!(
            config
                .client()
                .unwrap()
                .get(&url)
                .send()
                .unwrap()
                .text()
                .unwrap(),
            "ok"
        );
        assert!(server.join().unwrap().starts_with("GET /local HTTP/1.1"));
        assert!(proxy.accept().is_err());
        assert_eq!(config.route(&url).unwrap(), None);
    }
}

#[test]
fn saved_settings_are_device_local_persist_and_reject_invalid_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    let engine = Engine::open(&path).unwrap();
    assert_eq!(
        engine.execute("network_settings", json!({})).unwrap()["settings"]["mode"],
        "system"
    );
    let version = engine.lock().unwrap().usage_version().unwrap();
    let view = engine
        .execute(
            "save_network_settings",
            json!({"settings":{"mode":"manual","proxyUrl":" http://127.0.0.1:7890 "}}),
        )
        .unwrap();
    assert_eq!(view["settings"]["proxyUrl"], "http://127.0.0.1:7890/");
    for route in view["routes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["configured"] == true)
    {
        assert_eq!(route["proxy"], "http://127.0.0.1:7890/");
    }
    for url in [
        "",
        "127.0.0.1:7890",
        "ftp://localhost",
        "http://user:secret@localhost",
        "http://localhost/path",
        "http://localhost/?secret=yes",
        "http://localhost/#frag",
        "http://local\nhost",
    ] {
        let error = engine
            .execute(
                "save_network_settings",
                json!({"settings":{"mode":"manual","proxyUrl":url}}),
            )
            .unwrap_err()
            .to_string();
        assert!(!error.contains("secret"));
        assert_eq!(
            engine.execute("network_settings", json!({})).unwrap()["settings"],
            view["settings"]
        );
    }
    for scheme in ["http", "https", "socks5", "socks5h"] {
        settings(Mode::Manual, &format!("{scheme}://[::1]:1080"))
            .validate()
            .unwrap();
    }
    assert_eq!(engine.lock().unwrap().usage_version().unwrap(), version);
    assert!(engine.lock().unwrap().pending().unwrap().is_empty());
    assert!(!engine
        .execute("snapshot", json!({}))
        .unwrap()
        .to_string()
        .contains("proxyUrl"));
    drop(engine);
    let engine = Engine::open(&path).unwrap();
    assert_eq!(
        engine.execute("network_settings", json!({})).unwrap()["settings"],
        view["settings"]
    );
}

#[test]
fn helper_proxy_environment_is_scoped_to_child_and_direct_disables_inherited_proxies() {
    let before = std::env::var_os("HTTPS_PROXY");
    for (mode, bypass, proxy) in [
        (Mode::Manual, LOOPBACK, "http://127.0.0.1:7890/"),
        (Mode::Direct, "*", ""),
    ] {
        let mut child = Command::new("unused-helper");
        settings(mode, "http://127.0.0.1:7890")
            .configure_command(&mut child)
            .unwrap();
        let env: std::collections::BTreeMap<_, _> = child
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().to_ascii_uppercase(),
                    v.unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            assert_eq!(env[key], proxy);
        }
        assert_eq!(env["NO_PROXY"], bypass);
    }
    assert_eq!(std::env::var_os("HTTPS_PROXY"), before);
}

#[test]
fn leaving_manual_mode_discards_an_invalid_hidden_proxy_draft() {
    for mode in [Mode::System, Mode::Direct] {
        let mut config = settings(mode, "http://user:secret@unfinished/");
        config.validate().unwrap();
        assert!(config.proxy_url.is_empty());
    }
}

#[test]
fn system_mode_uses_inherited_environment_in_an_isolated_process() {
    let proxy = listener();
    let address = format!("http://{}", proxy.local_addr().unwrap());
    let server = http_server(proxy, "200 OK");
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.args([
        "--exact",
        "network::tests::environment_proxy_child",
        "--ignored",
    ]);
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"] {
        child.env_remove(key).env_remove(key.to_ascii_lowercase());
    }
    child
        .env("HTTP_PROXY", &address)
        .env("HTTPS_PROXY", &address)
        .env("NO_PROXY", LOOPBACK)
        .env("SCOPE_TEST_PROXY", &address);
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(server
        .join()
        .unwrap()
        .starts_with("GET http://network-fixture.invalid/catalog HTTP/1.1"));
}

#[test]
#[ignore = "Spawned by system_mode_uses_inherited_environment_in_an_isolated_process"]
fn environment_proxy_child() {
    let address = std::env::var("SCOPE_TEST_PROXY").unwrap();
    let config = Settings::default();
    let target = "http://network-fixture.invalid/catalog";
    assert_eq!(config.route(target).unwrap(), Some(format!("{address}/")));
    assert_eq!(
        config
            .client()
            .unwrap()
            .get(target)
            .send()
            .unwrap()
            .text()
            .unwrap(),
        "ok"
    );
    assert_eq!(config.route("http://127.0.0.1/").unwrap(), None);
}
