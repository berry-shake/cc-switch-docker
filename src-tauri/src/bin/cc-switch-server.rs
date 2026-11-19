//! 无界面的用量统计服务入口，见 `cc_switch_lib::web_server`。
//!
//! cargo build --release --features web-server --bin cc-switch-server

fn main() {
    std::process::exit(cc_switch_lib::run_web_server());
}
