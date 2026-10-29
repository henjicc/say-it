pub mod indicator;
pub mod orb;
pub mod subtitle;

pub(crate) fn register() {
    orb::register();
    subtitle::register();
}
