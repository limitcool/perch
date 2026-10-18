fn main() {
    // 把图标和版本信息嵌入 exe(资源管理器/任务栏/托盘都用它)
    if std::env::var("CARGO_CFG_TARGET_OS").map(|o| o == "windows").unwrap_or(false) {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/perch.ico");
        res.set("ProductName", "Perch");
        res.set("FileDescription", "Perch desktop HUD");
        res.set("CompanyName", "Perch");
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed icon: {}", e);
        }
    }
}
