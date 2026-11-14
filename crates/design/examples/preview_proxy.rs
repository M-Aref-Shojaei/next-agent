//! Run the edit-mode preview proxy by hand, for checking the bridge in a
//! browser without the desktop app:
//!
//! ```sh
//! cargo run -p next-agent-design --example preview_proxy -- http://localhost:5173 http://localhost:8000
//! ```
//!
//! The first argument is the dev server (loopback only), the second the
//! origin of the page that frames the preview (the bridge only talks to it).

use next_agent_design::proxy::{bridge_script, PreviewProxy};
use next_agent_design::ssrf::check_preview_url;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let target = args
        .next()
        .ok_or("usage: preview_proxy <dev-server-url> <studio-origin>")?;
    let origin = args
        .next()
        .ok_or("usage: preview_proxy <dev-server-url> <studio-origin>")?;
    let target = check_preview_url(&target).map_err(|e| e.to_string())?;
    let bridge = bridge_script(&origin).map_err(|e| e.to_string())?;
    let proxy = PreviewProxy::start(target, bridge).map_err(|e| e.to_string())?;
    println!("{}", proxy.url());
    loop {
        std::thread::park();
    }
}
