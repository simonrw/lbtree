use aws_config::SdkConfig;
use aws_sdk_apigateway::types::{Integration, Method, Resource, RestApi};
use color_eyre::eyre::{self, Context};
use crossbeam::channel::unbounded;
use skim::prelude::*;
use std::borrow::Cow;
use std::sync::Arc;

use crate::present::{OutputWriter, Present};

#[derive(Debug, Clone)]
struct RestApiItem {
    display: String, // What user sees: "name (id)"
    id: String,      // What gets returned when selected
}

impl SkimItem for RestApiItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display)
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.id)
    }
}

impl Present for RestApi {
    fn content(&self) -> String {
        format!(
            "REST API \"{}\" ({})",
            self.name().unwrap_or("unknown"),
            self.id().unwrap_or("unknown")
        )
    }

    fn indent(&self) -> usize {
        0
    }
}

impl Present for Resource {
    fn content(&self) -> String {
        format!(
            "{} (id={})",
            self.path().unwrap_or("/"),
            self.id().unwrap_or("unknown")
        )
    }

    fn indent(&self) -> usize {
        2
    }
}

impl Present for Method {
    fn content(&self) -> String {
        format!(
            "{} auth={}",
            self.http_method().unwrap_or("unknown"),
            self.authorization_type().unwrap_or("NONE")
        )
    }

    fn indent(&self) -> usize {
        4
    }
}

impl Present for Integration {
    fn content(&self) -> String {
        let integration_type = self
            .r#type()
            .map(|t| format!("{:?}", t))
            .unwrap_or("unknown".to_string());
        let uri = self.uri().unwrap_or("none");
        format!("Integration type={} uri={}", integration_type, uri)
    }

    fn indent(&self) -> usize {
        6
    }
}

/// Let the user choose the REST API to use
async fn select_rest_api(client: &aws_sdk_apigateway::Client) -> eyre::Result<Option<String>> {
    // Create crossbeam channel for streaming items to skim
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    // Clone client for background task
    let client = client.clone();

    // Spawn background task to fetch and stream REST APIs
    let fetch_handle = tokio::spawn(async move {
        let result: eyre::Result<()> = async {
            // Fetch REST APIs (API Gateway doesn't have a paginator for get_rest_apis)
            let response = client
                .get_rest_apis()
                .send()
                .await
                .context("fetching REST APIs")?;

            // Send each API to skim immediately
            for api in response.items() {
                let name = api.name().unwrap_or("unknown");
                let id = api.id().unwrap_or("");

                let item = RestApiItem {
                    display: format!("{} ({})", name, id),
                    id: id.to_string(),
                };

                // Send to skim (crossbeam send is fast)
                // Ignore send errors - means user closed skim early
                let _ = tx.send(Arc::new(item));
            }

            Ok(())
        }
        .await;

        // Drop sender to signal EOF to skim
        drop(tx);

        result
    });

    // Configure skim options
    let options = SkimOptionsBuilder::default()
        .height("50%".to_string())
        .prompt("Select REST API: ".to_string())
        .build()
        .map_err(|e| eyre::eyre!("building skim options: {}", e))?;

    // Start skim UI immediately (receives items as they arrive)
    let selected = Skim::run_with(&options, Some(rx));

    // Wait for background task and check for errors
    let fetch_result = fetch_handle
        .await
        .context("background fetch task panicked")?;

    // Propagate any AWS API errors
    fetch_result?;

    // Extract selection
    let selected = match selected {
        Some(output) => {
            if output.is_abort {
                return Ok(None);
            }

            output
                .selected_items
                .first()
                .map(|item| item.output().to_string())
        }
        None => None,
    };

    Ok(selected)
}

/// Display an API Gateway REST API hierarchy
pub async fn display_apigateway(
    config: &SdkConfig,
    api_id: Option<String>,
    writer: &dyn OutputWriter,
) -> eyre::Result<()> {
    let client = aws_sdk_apigateway::Client::new(config);

    let api_id = if let Some(id) = api_id {
        id
    } else {
        match select_rest_api(&client).await? {
            Some(id) => id,
            None => {
                eprintln!("No REST API selected");
                std::process::exit(1);
            }
        }
    };

    // Fetch the REST API
    let api = client
        .get_rest_api()
        .rest_api_id(&api_id)
        .send()
        .await
        .context("fetching REST API")?;

    // Present the REST API
    let rest_api = RestApi::builder()
        .set_id(api.id().map(|s| s.to_string()))
        .set_name(api.name().map(|s| s.to_string()))
        .build();
    rest_api.present(writer);

    // Fetch all resources for this API
    let resources_response = client
        .get_resources()
        .rest_api_id(&api_id)
        .send()
        .await
        .context("fetching resources")?;

    // Process each resource
    for resource in resources_response.items() {
        resource.present(writer);

        // Process methods for this resource
        if let Some(methods) = resource.resource_methods() {
            for (http_method, method_obj) in methods {
                method_obj.present(writer);

                // Fetch integration for this method
                let integration_result = client
                    .get_integration()
                    .rest_api_id(&api_id)
                    .resource_id(resource.id().unwrap_or(""))
                    .http_method(http_method)
                    .send()
                    .await;

                match integration_result {
                    Ok(integration) => {
                        let integration_obj = Integration::builder()
                            .set_type(integration.r#type().cloned())
                            .set_uri(integration.uri().map(|s| s.to_string()))
                            .build();
                        integration_obj.present(writer);
                    }
                    Err(e) => {
                        // Some methods might not have integrations, just skip
                        eprintln!(
                            "Warning: Could not fetch integration for {} {}: {}",
                            resource.path().unwrap_or("unknown"),
                            http_method,
                            e
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
