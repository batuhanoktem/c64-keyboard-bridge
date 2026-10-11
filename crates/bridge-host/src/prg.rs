use crate::pairing::Token;
use anyhow::{Result, bail};
use std::net::Ipv4Addr;
const MARKER: &[u8; 8] = b"KBCFGv1\0";
pub const CONFIG_LEN: usize = 64;

pub fn patch(
    template: &[u8],
    host: Ipv4Addr,
    port: u16,
    token: Token,
    device_id: &str,
) -> Result<Vec<u8>> {
    if template.len() < CONFIG_LEN + 2 || template.len() > 65537 || template[..2] != [1, 8] {
        bail!("Template is not a supported C64 PRG with load address $0801");
    }
    if host.is_unspecified() || host.is_multicast() || host.is_broadcast() || port == 0 {
        bail!("PRG destination must be a unicast IPv4 address and nonzero port");
    }
    if device_id.is_empty()
        || device_id.len() > 19
        || !device_id.bytes().all(|b| (32..=126).contains(&b))
    {
        bail!("Device identity must be 1–19 printable ASCII bytes for this C64 binary");
    }
    let offsets: Vec<_> = template
        .windows(MARKER.len())
        .enumerate()
        .filter_map(|(i, b)| (b == MARKER).then_some(i))
        .collect();
    if offsets.len() != 1 {
        bail!("PRG configuration marker is absent or ambiguous");
    }
    let offset = offsets[0];
    if offset + CONFIG_LEN > template.len()
        || template[offset + 8] != 1
        || template[offset + 9] != CONFIG_LEN as u8
    {
        bail!("PRG configuration block has an unsupported version or size");
    }
    let mut bytes = template.to_vec();
    let cfg = &mut bytes[offset..offset + CONFIG_LEN];
    cfg[10..26].fill(0);
    let address = host.to_string();
    cfg[10..10 + address.len()].copy_from_slice(address.as_bytes());
    cfg[26..28].copy_from_slice(&port.to_le_bytes());
    cfg[28..44].copy_from_slice(&token.0);
    cfg[44] = device_id.len() as u8;
    cfg[45..64].fill(0);
    cfg[45..45 + device_id.len()].copy_from_slice(device_id.as_bytes());
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn template() -> Vec<u8> {
        let mut data = vec![1, 8];
        data.extend(MARKER);
        data.extend([1, 64]);
        data.resize(66, 0);
        data
    }
    #[test]
    fn patch_preserves_binary_and_offsets() {
        let base = template();
        let p = patch(
            &base,
            Ipv4Addr::new(10, 1, 2, 3),
            6502,
            Token([42; 16]),
            "C64-Ultimate",
        )
        .unwrap();
        assert_eq!(&p[12..20], b"10.1.2.3");
        assert_eq!(&p[28..30], &6502u16.to_le_bytes());
        assert_eq!(&p[30..46], &[42; 16]);
        assert_eq!(base[30], 0);
    }
    #[test]
    fn reject_ambiguous_and_invalid_templates() {
        let mut t = template();
        t.extend(MARKER);
        assert!(patch(&t, Ipv4Addr::LOCALHOST, 6502, Token([0; 16]), "C64").is_err());
        assert!(patch(&[0; 70], Ipv4Addr::LOCALHOST, 6502, Token([0; 16]), "C64").is_err());
    }
}
