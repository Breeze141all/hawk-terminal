fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resource: {e}");
        }

        if let Ok(out_dir) = std::env::var("OUT_DIR") {
            let res_lib = std::path::Path::new(&out_dir).join("resource.lib");
            if res_lib.exists() {
                // If resource.lib starts with RES header (0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00),
                // MSVC link.exe requires a .res extension to recognize it as a compiled resource.
                let data = std::fs::read(&res_lib).unwrap_or_default();
                let is_res = data.starts_with(&[0, 0, 0, 0, 0x20, 0, 0, 0]);
                let target_path = if is_res {
                    let res_file = std::path::Path::new(&out_dir).join("resource.res");
                    let _ = std::fs::write(&res_file, &data);
                    res_file
                } else {
                    res_lib
                };
                println!(
                    "cargo:rustc-link-arg-bin=hawk-terminal={}",
                    target_path.display()
                );
            }
        }
    }
}
