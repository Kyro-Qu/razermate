use std::path::PathBuf;
use std::process::Command;

fn main() {
    slint_build::compile("ui/app.slint").expect("编译 ui/app.slint 失败");
    println!("cargo:rerun-if-changed=ui/app.slint");

    // 应用图标 / 文件属性：资源只在 Windows 目标上编译。
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources();
    }
}

/// 把 `assets/app.rc` 编成 COFF 目标文件再交给链接器。
///
/// 项目跑在 `x86_64-pc-windows-gnu` 上，工具链自带 MinGW 的 `windres`；
/// 资源对象只是纯数据段，和 rustc 生成的 .o 混链没有问题。
/// 找不到 `windres`（例如换成 MSVC 目标）时只警告、不中断构建——
/// 图标属于装饰，不该让整个编译失败。
fn embed_windows_resources() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let rc = manifest.join("assets").join("app.rc");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app_res.o");

    let result = Command::new("windres")
        .arg("--include-dir")
        .arg(manifest.join("assets"))
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&out)
        .arg(&rc)
        .output();

    match result {
        Ok(o) if o.status.success() => {
            println!("cargo:rustc-link-arg={}", out.display());
        }
        Ok(o) => println!(
            "cargo:warning=windres 编译资源失败，跳过图标嵌入：{}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => println!("cargo:warning=找不到 windres，跳过图标嵌入：{e}"),
    }
}
