//! Read-only MD5 inventory for a locally supplied GoldSrc client DLL.
use md5::{Digest, Md5};
use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const MAX_BYTES: u64 = 256 * 1024 * 1024;

fn digest_reader(mut reader: impl Read, max_bytes: u64) -> io::Result<([u8; 16], u64)> {
    let mut hasher = Md5::new();
    let mut buffer = [0_u8; 8192];
    let mut total = 0_u64;
    loop {
        let n = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            other => other?,
        };
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or_else(|| io::Error::other("file size overflow"))?;
        if total > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "client DLL exceeds size limit",
            ));
        }
        hasher.update(&buffer[..n]);
    }
    if total == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "client DLL is empty",
        ));
    }
    Ok((hasher.finalize().into(), total))
}

fn inspect(path: &Path) -> io::Result<([u8; 16], u64)> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected bounded regular file",
        ));
    }
    digest_reader(file, MAX_BYTES)
}

fn main() -> io::Result<()> {
    let mut args = env::args_os();
    let _program = args.next();
    let path = args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: client_dll_inventory <local-client.dll>",
        )
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one file path",
        ));
    }
    let (digest, size) = inspect(Path::new(&path))?;
    print!("client DLL bytes={size} md5=");
    for byte in digest {
        print!("{byte:02x}");
    }
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_md5_known_vector_and_rejects_empty_or_oversized() {
        let (digest, size) = digest_reader(&b"abc"[..], 3).unwrap();
        assert_eq!(
            digest,
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
                0x7f, 0x72
            ]
        );
        assert_eq!(size, 3);
        assert!(digest_reader(&b""[..], 3).is_err());
        assert!(digest_reader(&b"abcd"[..], 3).is_err());
    }

    #[test]
    fn reads_multiple_chunks_and_propagates_errors() {
        let data = vec![42; 20_000];
        let (digest, size) = digest_reader(data.as_slice(), 20_000).unwrap();
        assert_eq!(digest.as_slice(), Md5::digest(&data).as_slice());
        assert_eq!(size, 20_000);
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("test read failure"))
            }
        }
        assert!(digest_reader(Broken, MAX_BYTES).is_err());
    }
}
