fn main() {
    // Vulkan 运行库随显卡驱动安装，不是每台机器都有。延迟加载后进程（和测试）在没有它的机器上也能启动，
    // 不会被系统弹「找不到 vulkan-1.dll」；主程序在启动推理前自己检查它在不在。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/DELAYLOAD:vulkan-1.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
}
