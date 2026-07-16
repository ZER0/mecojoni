use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=MECO_EMBEDDED_ARTIFACT");
    let output =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("embedded.mecob");
    let Some(authored) = env::var_os("MECO_EMBEDDED_ARTIFACT") else {
        fs::write(output, []).expect("write empty generic embedded slot");
        return;
    };
    let authored = PathBuf::from(authored);
    let path = if authored.is_absolute() {
        authored
    } else {
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"))
            .join(authored)
    };
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read embedded artifact {}: {error}",
            path.display()
        )
    });
    assert!(
        bytes.starts_with(b"MECB"),
        "embedded artifact {} does not have MECB magic",
        path.display()
    );
    fs::write(output, bytes).expect("copy embedded artifact into OUT_DIR");
}
