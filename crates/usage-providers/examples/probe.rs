use usage_core::{Measurement, Provider};
use usage_providers::ProviderClient;
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--credential-helper"] {
        usage_providers::credential_helper_main();
    }
    let provider = match args.first().map(String::as_str) {
        Some("codex") => Provider::Codex,
        Some("claude") => Provider::Claude,
        Some("grok") => Provider::GrokBot,
        Some("antigravity") => Provider::Antigravity,
        _ => {
            eprintln!("usage: probe <codex|claude|grok|antigravity> [--values]");
            std::process::exit(2);
        }
    };
    let values = args.iter().any(|arg| arg == "--values");
    let client = match ProviderClient::new() {
        Ok(c) => c,
        Err(e) => {
            println!("client_status={e:?}");
            return;
        }
    };
    let probe = match client.prepare(provider).await {
        Ok(p) => p,
        Err(e) => {
            println!("prepare_status={:?}", e.error);
            return;
        }
    };
    println!("prepare_status=ready identity=redacted");
    match client.fetch(probe).await {
        Ok(snapshot) => {
            println!("fetch_status=ok windows={}", snapshot.windows.len());
            for (index, w) in snapshot.windows.iter().enumerate() {
                println!(
                    "window={index} measured={} period_present={} reset_present={} trial_expiry_present={}",
                    w.measurement != Measurement::Unknown,
                    w.period.is_some(),
                    w.resets_at.is_some(),
                    w.trial_expires_at.is_some()
                );
                if values && let Measurement::Percent { value, direction } = w.measurement {
                    println!(
                        "window={index} percent={} direction={direction:?}",
                        value.get()
                    );
                }
            }
        }
        Err(e) => println!(
            "fetch_status={:?} retry_after_present={} identity_invalidated={}",
            e.error,
            e.retry_after.is_some(),
            e.identity_invalidated
        ),
    }
}
