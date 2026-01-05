use aws_config::SdkConfig;
use aws_sdk_elasticloadbalancingv2::config::Credentials;

/// Create an AWS config that points to LocalStack
pub async fn localstack_config() -> SdkConfig {
    aws_config::from_env()
        .endpoint_url("http://localhost:4566")
        .credentials_provider(Credentials::new(
            "test",
            "test",
            None,
            None,
            "test-provider",
        ))
        .load()
        .await
}

/// Check if LocalStack is available at localhost:4566
pub async fn is_localstack_available() -> bool {
    // Check if LocalStack port is open by attempting a TCP connection
    use std::net::TcpStream;
    use std::time::Duration;

    TcpStream::connect_timeout(
        &"127.0.0.1:4566".parse().unwrap(),
        Duration::from_millis(500),
    )
    .is_ok()
}

/// Skip test if LocalStack is not available
#[macro_export]
macro_rules! assert_localstack_available {
    () => {
        assert!(
            common::is_localstack_available().await,
            "LocalStack is not available"
        );
    };
}
