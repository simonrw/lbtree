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
    // Check LocalStack health endpoint
    let health_url = "http://localhost:4566/_localstack/health";

    match reqwest::get(health_url).await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

/// Skip test if LocalStack is not available
#[macro_export]
macro_rules! skip_if_localstack_unavailable {
    () => {
        if !common::is_localstack_available().await {
            eprintln!("Skipping test: LocalStack not available at http://localhost:4566");
            eprintln!("Start LocalStack with: docker run --rm -d -p 4566:4566 localstack/localstack");
            return;
        }
    };
}
