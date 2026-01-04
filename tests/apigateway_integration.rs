mod common;

use std::time::Duration;

use aws_sdk_apigateway::Client as ApiGatewayClient;
use lbtree::present::BufferWriter;
use uuid::Uuid;

struct ApiGatewayTestFixture {
    config: aws_config::SdkConfig,
    client: ApiGatewayClient,
    api_id: Option<String>,
    resource_ids: Vec<String>,
    insta_settings: insta::Settings,
}

impl ApiGatewayTestFixture {
    async fn new() -> color_eyre::Result<Self> {
        let config = common::localstack_config().await;
        let client = ApiGatewayClient::new(&config);

        let mut fixture = Self {
            config,
            client,
            api_id: None,
            resource_ids: Vec::new(),
            insta_settings: insta::Settings::clone_current(),
        };

        fixture.setup().await?;
        Ok(fixture)
    }

    async fn setup(&mut self) -> color_eyre::Result<()> {
        let test_id = Uuid::new_v4();
        let api_name = format!("test-api-{}", test_id);
        self.insta_settings.add_filter(&api_name, "[api-name]");

        // Create REST API
        let api = self
            .client
            .create_rest_api()
            .name(&api_name)
            .description("Test REST API")
            .send()
            .await?;
        self.api_id = api.id().map(|s| s.to_string());
        let api_id = self.api_id.as_ref().unwrap();
        self.insta_settings.add_filter(&api_id, "[api-id]");

        // Get the root resource
        let resources = self
            .client
            .get_resources()
            .rest_api_id(api_id)
            .send()
            .await?;

        let root_id = resources
            .items()
            .iter()
            .find(|r| r.path() == Some("/"))
            .and_then(|r| r.id())
            .unwrap();
        self.insta_settings.add_filter(root_id, "[root-id]");

        // Create a resource path /users
        let users_resource = self
            .client
            .create_resource()
            .rest_api_id(api_id)
            .parent_id(root_id)
            .path_part("users")
            .send()
            .await?;
        if let Some(id) = users_resource.id().map(|s| s.to_string()) {
            self.insta_settings.add_filter(&id, "[users-resource-id]");
            self.resource_ids.push(id.clone());

            // Create GET method on /users
            let _ = self
                .client
                .put_method()
                .rest_api_id(api_id)
                .resource_id(&id)
                .http_method("GET")
                .authorization_type("NONE")
                .send()
                .await?;

            // Create integration for GET /users
            let _ = self
                .client
                .put_integration()
                .rest_api_id(api_id)
                .resource_id(&id)
                .http_method("GET")
                .integration_http_method("GET")
                .r#type(aws_sdk_apigateway::types::IntegrationType::Mock)
                .send()
                .await?;
        }

        // Create a resource path /products
        let products_resource = self
            .client
            .create_resource()
            .rest_api_id(api_id)
            .parent_id(root_id)
            .path_part("products")
            .send()
            .await?;
        if let Some(id) = products_resource.id().map(|s| s.to_string()) {
            self.insta_settings
                .add_filter(&id, "[products-resource-id]");
            self.resource_ids.push(id.clone());

            // Create POST method on /products
            let _ = self
                .client
                .put_method()
                .rest_api_id(api_id)
                .resource_id(&id)
                .http_method("POST")
                .authorization_type("NONE")
                .send()
                .await?;

            // Create integration for POST /products
            let _ = self
                .client
                .put_integration()
                .rest_api_id(api_id)
                .resource_id(&id)
                .http_method("POST")
                .integration_http_method("POST")
                .r#type(aws_sdk_apigateway::types::IntegrationType::Http)
                .uri("http://example.com/products")
                .send()
                .await?;
        }

        Ok(())
    }

    async fn run_display(&self) -> color_eyre::Result<String> {
        let writer = BufferWriter::new();
        lbtree::apigateway::display_apigateway(&self.config, self.api_id.clone(), &writer).await?;
        Ok(writer.get_output())
    }

    async fn cleanup(&mut self) {
        // Delete REST API (this cascades to all resources, methods, and integrations)
        if let Some(id) = &self.api_id {
            let _ = self.client.delete_rest_api().rest_api_id(id).send().await;
        }
    }
}

#[tokio::test]
async fn test_apigateway_display_snapshot() {
    assert_localstack_available!();

    let mut fixture = ApiGatewayTestFixture::new()
        .await
        .expect("Failed to create test fixture");
    let output = fixture.run_display().await;

    fixture.cleanup().await;
    let output = output.expect("error with test");

    // Use insta for snapshot testing
    fixture.insta_settings.bind(|| {
        insta::assert_snapshot!(output);
    });
}
