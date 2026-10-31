//! Offline diagnostic for HLDS meta-delta descriptions. Not used by the server.
use std::env;
use std::fs;
use std::io::{self, Error, ErrorKind};

const LIMIT: usize = 65_536;

#[derive(Debug)]
struct Field {
    name: String,
    kind: u32,
    offset: u16,
    size: u8,
    bits: u8,
    pre: f32,
    post: f32,
}

#[derive(Debug)]
struct Schema {
    name: String,
    fields: Vec<Field>,
}

fn invalid() -> Error {
    Error::new(ErrorKind::InvalidData, "invalid delta description input")
}

fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 31 && name.bytes().all(|b| b.is_ascii_graphic())
}

fn parse(input: &str) -> io::Result<Vec<Schema>> {
    if input.len() > LIMIT {
        return Err(invalid());
    }
    let mut lines = input.lines();
    let mut schemas = Vec::new();
    while let Some(line) = lines.next() {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() != 3 || parts[0] != "schema" || !name_ok(parts[1]) || schemas.len() >= 32 {
            return Err(invalid());
        }
        let count: usize = parts[2].parse().map_err(|_| invalid())?;
        if count == 0 || count > 64 {
            return Err(invalid());
        }
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            let parts: Vec<_> = lines.next().ok_or_else(invalid)?.split('\t').collect();
            if parts.len() != 8 || parts[0] != "field" || !name_ok(parts[1]) {
                return Err(invalid());
            }
            let field = Field {
                name: parts[1].to_owned(),
                kind: parts[2].parse().map_err(|_| invalid())?,
                offset: parts[3].parse().map_err(|_| invalid())?,
                size: parts[4].parse().map_err(|_| invalid())?,
                bits: parts[5].parse().map_err(|_| invalid())?,
                pre: parts[6].parse().map_err(|_| invalid())?,
                post: parts[7].parse().map_err(|_| invalid())?,
            };
            if !field.pre.is_finite()
                || !field.post.is_finite()
                || field.pre < 0.0
                || field.post < 0.0
                || f64::from(field.pre) * 4000.0 > f64::from(u32::MAX)
                || f64::from(field.post) * 4000.0 > f64::from(u32::MAX)
            {
                return Err(invalid());
            }
            fields.push(field);
        }
        schemas.push(Schema {
            name: parts[1].to_owned(),
            fields,
        });
    }
    if schemas.is_empty() {
        return Err(invalid());
    }
    Ok(schemas)
}

struct Writer {
    data: Vec<u8>,
    bit: usize,
}

impl Writer {
    fn bits(&mut self, value: u32, count: usize) -> io::Result<()> {
        for i in 0..count {
            if self.bit / 8 >= LIMIT {
                return Err(invalid());
            }
            if self.bit.is_multiple_of(8) {
                self.data.push(0);
            }
            self.data[self.bit / 8] |= (((value >> i) & 1) as u8) << (self.bit % 8);
            self.bit += 1;
        }
        Ok(())
    }

    fn string(&mut self, value: &str) -> io::Result<()> {
        for b in value.bytes().chain(std::iter::once(0)) {
            self.bits(u32::from(b), 8)?;
        }
        Ok(())
    }

    fn align(&mut self) {
        self.bit = self.bit.next_multiple_of(8);
    }
}

fn encode(schemas: &[Schema]) -> io::Result<Vec<u8>> {
    let mut w = Writer {
        data: Vec::new(),
        bit: 0,
    };
    for schema in schemas {
        w.bits(14, 8)?;
        w.string(&schema.name)?;
        w.bits(schema.fields.len() as u32, 16)?;
        for field in &schema.fields {
            let values = [
                field.kind != 0,
                !field.name.is_empty(),
                field.offset != 0,
                field.size != 0,
                field.bits != 0,
                field.pre != 0.0,
                field.post != 0.0,
            ];
            let mut mask = 0_u32;
            for (index, present) in values.iter().enumerate() {
                if *present {
                    mask |= 1 << index;
                }
            }
            w.bits(u32::from(mask != 0), 3)?;
            if mask != 0 {
                w.bits(mask, 8)?;
            }
            if values[0] {
                w.bits(field.kind, 32)?;
            }
            if values[1] {
                w.string(&field.name)?;
            }
            if values[2] {
                w.bits(u32::from(field.offset), 16)?;
            }
            if values[3] {
                w.bits(u32::from(field.size), 8)?;
            }
            if values[4] {
                w.bits(u32::from(field.bits), 8)?;
            }
            if values[5] {
                w.bits((f64::from(field.pre) * 4000.0) as u32, 32)?;
            }
            if values[6] {
                w.bits((f64::from(field.post) * 4000.0) as u32, 32)?;
            }
        }
        w.align();
    }
    Ok(w.data)
}

fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "usage: delta_reference <fields.tsv> <hlds.bin>",
        ));
    }
    let input = fs::read_to_string(&args[1])?;
    let schemas = parse(&input)?;
    let expected = fs::read(&args[2])?;
    let actual = encode(&schemas)?;
    if actual != expected {
        let first = actual
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or(actual.len().min(expected.len()));
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "reference mismatch at byte {first}; actual={} expected={}",
                actual.len(),
                expected.len()
            ),
        ));
    }
    let fields: usize = schemas.iter().map(|s| s.fields.len()).sum();
    println!(
        "HLDS match: {} bytes, {} schemas, {fields} fields",
        actual.len(),
        schemas.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_input_and_truncation() {
        assert!(parse("").is_err());
        assert!(parse("schema\tx\t1\n").is_err());
        assert!(parse("schema\tx\t65\n").is_err());
        assert!(parse("schema\tx\t1\nfield\tx\t8\t0\t1\t8\tNaN\t1\n").is_err());
    }

    #[test]
    fn writes_bits_lsb_first_across_bytes() {
        let mut w = Writer {
            data: Vec::new(),
            bit: 0,
        };
        w.bits(5, 3).unwrap();
        w.bits(0xaa, 8).unwrap();
        assert_eq!(w.data, [0x55, 0x05]);
    }

    #[test]
    #[ignore = "requires separately acquired local HLDS field and binary captures"]
    fn matches_local_hlds_capture() {
        let root = std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR")
            .expect("set GOLDSRC_DELTA_REFERENCE_DIR to the local fixture directory");
        let root = std::path::Path::new(&root);
        let fields = fs::read_to_string(root.join("hlds-delta-fields-build10211.tsv")).unwrap();
        let expected = fs::read(root.join("hlds-delta-build10211.bin")).unwrap();
        let schemas = parse(&fields).unwrap();
        assert_eq!(schemas.len(), 7);
        assert_eq!(schemas.iter().map(|s| s.fields.len()).sum::<usize>(), 213);
        assert_eq!(encode(&schemas).unwrap(), expected);
    }
}
