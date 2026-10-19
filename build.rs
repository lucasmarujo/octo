use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

const ICON: &str = "assets/orkai.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const MAIN_ICON: u16 = 1;
const ICON_FLAGS: u16 = 0x1010;
const GROUP_FLAGS: u16 = 0x1030;
const ICO_HEADER: usize = 6;
const ICO_ENTRY: usize = 16;
const ENTRY_SHAPE: usize = 8;
const RESOURCE_HEADER: u32 = 32;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed={ICON}");
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|target_env| target_env == "msvc");
    if !windows || !msvc {
        return Ok(());
    }
    let out = env::var_os("OUT_DIR").map(PathBuf::from).ok_or_else(|| io::Error::other("OUT_DIR is not set"))?;
    let res = out.join("orkai.res");
    fs::write(&res, resource_file(&fs::read(ICON)?)?)?;
    println!("cargo:rustc-link-arg-bins={}", res.display());
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{ICON} is not a valid icon"))
}

fn read_u16(data: &[u8], at: usize) -> io::Result<u16> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(invalid)
}

fn read_u32(data: &[u8], at: usize) -> io::Result<usize> {
    let bytes = data.get(at..at + 4).ok_or_else(invalid)?;
    usize::try_from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).map_err(|_| invalid())
}

fn resource_file(icon: &[u8]) -> io::Result<Vec<u8>> {
    if read_u16(icon, 0)? != 0 || read_u16(icon, 2)? != 1 {
        return Err(invalid());
    }
    let count = read_u16(icon, 4)?;
    let mut out = Vec::new();
    push_resource(&mut out, 0, 0, 0, &[])?;
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&count.to_le_bytes());
    for index in 0..count {
        let entry = ICO_HEADER + usize::from(index) * ICO_ENTRY;
        let size = read_u32(icon, entry + 8)?;
        let offset = read_u32(icon, entry + 12)?;
        let image = icon.get(offset..offset + size).ok_or_else(invalid)?;
        let id = index + 1;
        push_resource(&mut out, RT_ICON, id, ICON_FLAGS, image)?;
        group.extend_from_slice(icon.get(entry..entry + ENTRY_SHAPE).ok_or_else(invalid)?);
        group.extend_from_slice(&u32::try_from(size).map_err(|_| invalid())?.to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
    }
    push_resource(&mut out, RT_GROUP_ICON, MAIN_ICON, GROUP_FLAGS, &group)?;
    Ok(out)
}

fn push_resource(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) -> io::Result<()> {
    out.extend_from_slice(&u32::try_from(data.len()).map_err(|_| invalid())?.to_le_bytes());
    out.extend_from_slice(&RESOURCE_HEADER.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
    Ok(())
}
