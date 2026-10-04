//! Execution semantics are verified independently of frontend/app attribution.
//! This template is the Sourcify-verified RH V3 pool runtime. Only compiler
//! metadata and documented immutable arguments are masked; changed opcodes fail.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Template {
    template_sha256: String,
    immutable_ranges: Vec<(usize, usize)>,
}

pub fn v3_topic() -> String {
    format!(
        "0x{:x}",
        Keccak256::digest(b"Swap(address,address,int256,int256,uint160,uint128,int24)")
    )
}

pub fn verified_v3_runtime(value: &str) -> bool {
    static TEMPLATE: OnceLock<Template> = OnceLock::new();
    let template = TEMPLATE.get_or_init(|| {
        serde_json::from_str(include_str!("v3-template.json"))
            .expect("Pinned V3 runtime template is valid")
    });
    let Some(hex) = value.strip_prefix("0x") else {
        return false;
    };
    if hex.len() % 2 != 0 || !hex.is_ascii() {
        return false;
    }
    let Some(mut code) = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        })
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if code.len() < 2 {
        return false;
    }
    let metadata = usize::from(u16::from_be_bytes([
        code[code.len() - 2],
        code[code.len() - 1],
    ])) + 2;
    if metadata >= code.len() {
        return false;
    }
    code.truncate(code.len() - metadata);
    for &(start, length) in &template.immutable_ranges {
        let Some(slice) = code.get_mut(start..start.saturating_add(length)) else {
            return false;
        };
        slice.fill(0);
    }
    format!("{:x}", Sha256::digest(&code)) == template.template_sha256
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_swap_topic_does_not_certify_forged_pool_code() {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/tracker-v3-runtime.json"))
                .unwrap();
        let code = value["observedBytecode"].as_str().unwrap();
        assert!(verified_v3_runtime(code));
        let changed = format!("0x00{}", &code[4..]);
        assert!(!verified_v3_runtime(&changed));
        assert!(!verified_v3_runtime("0x00"));
        assert_eq!(
            v3_topic(),
            "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"
        );
    }
}
