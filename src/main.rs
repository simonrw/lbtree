mod alb;
mod apigateway;
mod present;

use clap::{Parser, Subcommand};
use color_eyre::eyre;
use crossbeam::channel::unbounded;
use skim::prelude::*;
use std::borrow::Cow;
use std::sync::Arc;

use present::StdoutWriter;

#[derive(Parser)]
#[command(name = "lbtree")]
#[command(about = "Display AWS resource hierarchies as trees")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Display Application Load Balancer tree
    Alb {
        /// ARN of the load balancer (interactive selection if not provided)
        #[arg(short, long)]
        load_balancer_arn: Option<String>,
    },

    /// Display API Gateway REST API tree
    ApiGateway {
        /// ID of the REST API (interactive selection if not provided)
        #[arg(short = 'i', long)]
        api_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct ResourceTypeItem {
    display: String,
    resource_type: ResourceType,
}

#[derive(Debug, Clone)]
enum ResourceType {
    Alb,
    ApiGateway,
}

impl SkimItem for ResourceTypeItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display)
    }
}

/// Let the user choose the resource type to display
fn select_resource_type() -> eyre::Result<Option<ResourceType>> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    // Send resource type options
    let items = vec![
        ResourceTypeItem {
            display: "Application Load Balancer".to_string(),
            resource_type: ResourceType::Alb,
        },
        ResourceTypeItem {
            display: "API Gateway REST API".to_string(),
            resource_type: ResourceType::ApiGateway,
        },
    ];

    for item in items {
        let _ = tx.send(Arc::new(item));
    }
    drop(tx);

    // Configure skim options
    let options = SkimOptionsBuilder::default()
        .height(Some("50%"))
        .prompt(Some("Select resource type: "))
        .build()
        .map_err(|e| eyre::eyre!("building skim options: {}", e))?;

    // Start skim UI
    let selected = Skim::run_with(&options, Some(rx));

    // Extract selection
    let selected = match selected {
        Some(output) => {
            if output.is_abort {
                return Ok(None);
            }

            output.selected_items.first().and_then(|item| {
                item.as_any()
                    .downcast_ref::<ResourceTypeItem>()
                    .map(|i| i.resource_type.clone())
            })
        }
        None => None,
    };

    Ok(selected)
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    let cli = Cli::parse();
    let config = aws_config::load_from_env().await;
    let writer = StdoutWriter;

    match cli.command {
        Some(Commands::Alb { load_balancer_arn }) => {
            alb::display_alb(&config, load_balancer_arn, &writer).await?;
        }
        Some(Commands::ApiGateway { api_id }) => {
            apigateway::display_apigateway(&config, api_id, &writer).await?;
        }
        None => {
            // No subcommand provided, show resource type selection
            match select_resource_type()? {
                Some(ResourceType::Alb) => {
                    alb::display_alb(&config, None, &writer).await?;
                }
                Some(ResourceType::ApiGateway) => {
                    apigateway::display_apigateway(&config, None, &writer).await?;
                }
                None => {
                    eprintln!("No resource type selected");
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
