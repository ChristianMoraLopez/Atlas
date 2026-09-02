fn main() {
    println!("cargo:rerun-if-env-changed=CIRCANA_AZURE_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=CIRCANA_AZURE_TENANT_ID");
    tauri_build::build()
}
