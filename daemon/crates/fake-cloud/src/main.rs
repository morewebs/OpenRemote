//! Runs the stand-in on a loopback port and prints the environment that
//! points a daemon at it, for trying Cloud mode without moreweb.

#[tokio::main]
async fn main() {
    let cloud = fake_cloud::FakeCloud::start().await;
    println!("export OPENREMOTE_CLOUD_API={}", cloud.api());
    println!("export OPENREMOTE_AUTH_ISSUER={}", cloud.issuer());
    eprintln!(
        "fake-cloud: serving on {} (signs in as user@example.test)",
        cloud.base
    );
    std::future::pending::<()>().await;
}
