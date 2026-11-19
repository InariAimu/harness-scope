//! Bounded protobuf wire reader. Field numbers are from the descriptors shipped
//! in Antigravity 44.9.1 (gemini_coder.Step and exa.cortex_pb metadata).
//! Unknown fields are skipped; corrupt fields never become default token counts.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy)]
enum Field<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Fixed,
}
pub(super) struct Message<'a>(Vec<(u32, Field<'a>)>);

fn varint(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*cursor).context("Truncated protobuf varint")?;
        *cursor += 1;
        ensure!(shift != 63 || b <= 1, "Protobuf varint overflow");
        value |= u64::from(b & 127) << shift;
        if b < 128 {
            return Ok(value);
        }
    }
    anyhow::bail!("Protobuf varint overflow")
}

impl<'a> Message<'a> {
    pub(super) fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 32 * 1024 * 1024,
            "Antigravity metadata exceeds 32 MiB"
        );
        let mut out = Vec::new();
        let mut cursor = 0;
        while cursor < bytes.len() {
            ensure!(out.len() < 100_000, "Too many protobuf fields");
            let key = varint(bytes, &mut cursor)?;
            let number = u32::try_from(key >> 3)?;
            ensure!(
                (1..=536_870_911).contains(&number),
                "Invalid protobuf field number"
            );
            let wire = key & 7;
            let field = if wire == 0 {
                Field::Number(varint(bytes, &mut cursor)?)
            } else {
                let len = match wire {
                    1 => 8,
                    5 => 4,
                    2 => usize::try_from(varint(bytes, &mut cursor)?)?,
                    _ => anyhow::bail!("Unsupported protobuf wire type"),
                };
                let end = cursor
                    .checked_add(len)
                    .filter(|n| *n <= bytes.len())
                    .context("Truncated protobuf field")?;
                let slice = &bytes[cursor..end];
                cursor = end;
                if wire == 2 {
                    Field::Bytes(slice)
                } else {
                    Field::Fixed
                }
            };
            out.push((number, field));
        }
        Ok(Self(out))
    }
    pub(super) fn has(&self, number: u32) -> bool {
        self.0.iter().any(|(n, _)| *n == number)
    }
    pub(super) fn bytes(&self, number: u32) -> Result<&'a [u8]> {
        match self
            .0
            .iter()
            .rev()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| f)
        {
            None => Ok(&[]),
            Some(Field::Bytes(b)) => Ok(b),
            _ => anyhow::bail!("Unexpected protobuf field type"),
        }
    }
    pub(super) fn number(&self, number: u32) -> Result<u64> {
        match self
            .0
            .iter()
            .rev()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| f)
        {
            None => Ok(0),
            Some(Field::Number(v)) => Ok(*v),
            _ => anyhow::bail!("Unexpected protobuf number type"),
        }
    }
    pub(super) fn text(&self, number: u32) -> Result<&'a str> {
        Ok(std::str::from_utf8(self.bytes(number)?)?)
    }
    pub(super) fn message(&self, number: u32) -> Result<Message<'a>> {
        Self::parse(self.bytes(number)?)
    }
    pub(super) fn numbers(&self, number: u32) -> Result<Vec<u64>> {
        let mut out = Vec::new();
        for (_, field) in self.0.iter().filter(|(n, _)| *n == number) {
            match field {
                Field::Number(v) => out.push(*v),
                Field::Bytes(b) => {
                    let mut cursor = 0;
                    while cursor < b.len() {
                        out.push(varint(b, &mut cursor)?);
                    }
                }
                Field::Fixed => anyhow::bail!("Unexpected protobuf list type"),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncation_overflow_and_wrong_wire_types() {
        for bytes in [
            &[8, 128][..],
            &[10, 8, 1],
            &[0],
            &[8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2],
            &[11],
        ] {
            assert!(Message::parse(bytes).is_err());
        }
        assert!(Message::parse(&[10, 0]).unwrap().number(1).is_err());
        assert_eq!(
            Message::parse(&[18, 3, 1, 128, 1])
                .unwrap()
                .numbers(2)
                .unwrap(),
            vec![1, 128]
        );
    }
}
