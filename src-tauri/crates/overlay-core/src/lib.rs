//! 原生悬浮窗共用层：画什么、何时画、如何响应输入。不依赖任何平台 API。
pub mod anim;
pub mod blur;
pub mod canvas;
pub mod geom;
pub mod indicator;
pub mod look;
pub mod orb;
pub mod present;
pub mod screen;
pub mod subtitle;
pub mod svg;
#[cfg(test)]
mod testing;
pub mod window;
