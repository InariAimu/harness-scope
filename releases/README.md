# 最新二进制构建

本目录约定 Windows x64 桌面版和 Linux x64 同步服务端的构建产物位置。每个平台保留一份已验证的可执行程序。

| 平台目录 | 程序 | 用途 |
| --- | --- | --- |
| [windows-x86_64](windows-x86_64/) | `harness-scope-desktop.exe` | Windows 桌面应用，内嵌前端 |
| [linux-x86_64](linux-x86_64/) | `harness-scope-service` | Linux 同步服务器，命令行程序 |

各目录中的 `README.txt` 包含运行步骤和系统依赖。运行程序无需安装 Node.js 或 Rust，用户数据库、账号配置和凭据由使用者单独管理。其他平台的发布包见[项目首页的下载说明](../README.md#开始使用)。

`manifest.json` 记录各程序的构建身份、大小、SHA-256 和验证来源。查看 `channel` 和 `displayVersion` 可区分正式版本与开发构建。使用同批生成的 `SHA256SUMS.txt` 校验文件。

## 校验

在本目录运行：

```powershell
Get-FileHash .\windows-x86_64\harness-scope-desktop.exe -Algorithm SHA256
Get-FileHash .\linux-x86_64\harness-scope-service -Algorithm SHA256
```

Linux 可在本目录运行 `sha256sum --check SHA256SUMS.txt`。

## 更新约定

完成对应平台的构建和验收后，再更新同名程序及清单。不得用旧版本或其他平台程序代替缺失产物，也不在这里存放测试 EXE、PDB、编译缓存或数据库。替换旧程序时遵循仓库的可恢复清理规则。

二进制及生成清单由 `.gitignore` 排除，仓库跟踪各平台的说明文件。发布和部署需单独执行。
