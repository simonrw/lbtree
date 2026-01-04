use aws_config::SdkConfig;
use aws_sdk_elasticloadbalancingv2::types::{
    Action, ActionTypeEnum, Listener, LoadBalancer, Rule, TargetGroup, TargetHealthDescription,
};
use color_eyre::eyre::{self, Context};
use crossbeam::channel::unbounded;
use skim::prelude::*;
use std::borrow::Cow;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::present::{OutputWriter, Present};

#[derive(Debug, Clone)]
struct LoadBalancerItem {
    display: String, // What user sees: "name (dns-name)"
    arn: String,     // What gets returned when selected
}

impl SkimItem for LoadBalancerItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display)
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.arn)
    }
}

impl Present for LoadBalancer {
    fn content(&self) -> String {
        format!(
            "Load balancer ({dns_name})",
            dns_name = self.dns_name().unwrap()
        )
    }

    fn indent(&self) -> usize {
        0
    }
}

impl Present for Listener {
    fn content(&self) -> String {
        format!(
            "Listener protocol={protocol} port={port}",
            protocol = self.protocol().unwrap(),
            port = self.port().unwrap(),
        )
    }

    fn indent(&self) -> usize {
        2
    }
}

impl Present for Rule {
    fn content(&self) -> String {
        format!(
            "Rule priority={priority} is-default={is_default}",
            priority = self.priority().unwrap(),
            is_default = self.is_default().unwrap(),
        )
    }

    fn indent(&self) -> usize {
        4
    }
}

impl Present for Action {
    fn content(&self) -> String {
        match self.r#type().unwrap() {
            ActionTypeEnum::AuthenticateCognito => todo!("authenticate cognito"),
            ActionTypeEnum::AuthenticateOidc => todo!(),
            ActionTypeEnum::FixedResponse => {
                let cfg = self.fixed_response_config().unwrap();
                format!(
                    "Action (fixed-repsonse) msg={msg:?} status-code={status_code:?}",
                    msg = cfg.message_body(),
                    status_code = cfg.status_code(),
                )
            }
            ActionTypeEnum::Forward => {
                let _fwd = self.forward_config().unwrap();
                "Action (forward)".to_string()
            }
            ActionTypeEnum::Redirect => todo!(),
            _ => todo!(),
        }
    }

    fn indent(&self) -> usize {
        6
    }
}

impl Present for TargetGroup {
    fn content(&self) -> String {
        format!(
            "Target group \"{name}\" protocol={protocol} port={port}",
            name = self.target_group_name().unwrap_or("??"),
            protocol = self.protocol().unwrap(),
            port = self.port().unwrap()
        )
    }

    fn indent(&self) -> usize {
        2
    }
}

impl Present for TargetHealthDescription {
    fn content(&self) -> String {
        let target = self.target().unwrap();
        format!(
            "Target id={} port={}",
            target.id().unwrap(),
            target.port().unwrap()
        )
    }

    fn indent(&self) -> usize {
        4
    }
}

/// Let the user choose the load balancer to use
async fn select_load_balancer(
    client: &aws_sdk_elasticloadbalancingv2::Client,
) -> eyre::Result<Option<String>> {
    // Create crossbeam channel for streaming items to skim
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    // Clone client for background task
    let client = client.clone();

    // Spawn background task to fetch and stream load balancers
    let fetch_handle = tokio::spawn(async move {
        let result: eyre::Result<()> = async {
            // Use paginator to stream results as they arrive
            let mut paginator = client.describe_load_balancers().into_paginator().send();

            // Stream each page as it arrives from AWS
            while let Some(page) = paginator.next().await {
                let page = page.context("fetching load balancers page")?;

                // Send each LB to skim immediately
                for lb in page.load_balancers() {
                    let name = lb.load_balancer_name().unwrap_or("unknown");
                    let dns = lb.dns_name().unwrap_or("unknown");
                    let arn = lb.load_balancer_arn().unwrap_or("");

                    let item = LoadBalancerItem {
                        display: format!("{} ({})", name, dns),
                        arn: arn.to_string(),
                    };

                    // Send to skim (crossbeam send is fast)
                    // Ignore send errors - means user closed skim early
                    let _ = tx.send(Arc::new(item));
                }
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
        .prompt("Select load balancer: ".to_string())
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

/// Display an Application Load Balancer hierarchy
pub async fn display_alb(
    config: &SdkConfig,
    arn: Option<String>,
    writer: &dyn OutputWriter,
) -> eyre::Result<()> {
    let client = aws_sdk_elasticloadbalancingv2::Client::new(config);

    let lb_arn = if let Some(arn) = arn {
        arn
    } else {
        match select_load_balancer(&client).await? {
            Some(arn) => arn,
            None => {
                eprintln!("No load balancer selected");
                std::process::exit(1);
            }
        }
    };

    let load_balancer = client
        .describe_load_balancers()
        .load_balancer_arns(&lb_arn)
        .send()
        .await
        .context("describing load balancer")?;
    // panic safety: the client will return a 404 if the listener cannot be found, so we expect at
    // least one result
    let lb = &load_balancer.load_balancers()[0];
    lb.present(writer);

    // parallel fetch of the results

    let listeners_client = client.clone();
    let listeners_lb_arn = lb_arn.clone();
    let listeners_fut: JoinHandle<eyre::Result<Vec<Box<dyn Present>>>> = tokio::spawn(async move {
        let mut out: Vec<Box<dyn Present>> = Vec::new();

        let listeners = listeners_client
            .describe_listeners()
            .load_balancer_arn(listeners_lb_arn)
            .send()
            .await
            .wrap_err("describing listeners for load balancer")?;

        for listener in listeners.listeners() {
            out.push(Box::new(listener.clone()));

            let listener_arn = if let Some(arn) = listener.listener_arn() {
                arn
            } else {
                continue;
            };

            // - rules
            let rules = listeners_client
                .describe_rules()
                .listener_arn(listener_arn)
                .send()
                .await
                .context("describing rules for listener")?;

            for rule in rules.rules() {
                out.push(Box::new(rule.clone()));

                for action in rule.actions() {
                    out.push(Box::new(action.clone()));
                }
            }
        }

        Ok(out)
    });
    let target_groups_client = client.clone();
    let target_groups_lb_arn = lb_arn.clone();
    let target_groups_fut: JoinHandle<eyre::Result<Vec<Box<dyn Present>>>> =
        tokio::spawn(async move {
            let mut out: Vec<Box<dyn Present>> = Vec::new();
            let target_groups = target_groups_client
                .describe_target_groups()
                .load_balancer_arn(target_groups_lb_arn)
                .send()
                .await
                .context("describing target groups")?;

            for target_group in target_groups.target_groups() {
                out.push(Box::new(target_group.clone()));

                let tg_arn = if let Some(arn) = target_group.target_group_arn() {
                    arn
                } else {
                    continue;
                };

                // - targets
                let targets = target_groups_client
                    .describe_target_health()
                    .target_group_arn(tg_arn)
                    .send()
                    .await
                    .wrap_err("describing targets in target group")?;

                for target in targets.target_health_descriptions() {
                    out.push(Box::new(target.clone()));
                }
            }
            Ok(out)
        });

    for presenter in listeners_fut.await?? {
        presenter.present(writer);
    }
    for presenter in target_groups_fut.await?? {
        presenter.present(writer);
    }

    Ok(())
}
