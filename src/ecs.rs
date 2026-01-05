use aws_config::SdkConfig;
use aws_sdk_ecs::types::{Cluster, Service, Task};
use color_eyre::eyre::{self, Context};
use crossbeam::channel::unbounded;
use skim::prelude::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::present::{OutputWriter, Present};

#[derive(Debug, Clone)]
struct ClusterItem {
    display: String,
    arn: String,
}

impl SkimItem for ClusterItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display)
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.arn)
    }
}

#[derive(Debug, Clone)]
struct ServiceItem {
    display: String,
    arn: String,
}

impl SkimItem for ServiceItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display)
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.arn)
    }
}

/// Container information combining runtime state with definition
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub name: String,
    pub image: String,
    pub command: Option<Vec<String>>,
    pub last_status: Option<String>,
}

impl Present for Cluster {
    fn content(&self) -> String {
        let name = self.cluster_name().unwrap_or("unknown");
        let status = self.status().unwrap_or("unknown");
        let running_tasks = self.running_tasks_count();
        let pending_tasks = self.pending_tasks_count();
        let services = self.active_services_count();

        format!(
            "Cluster \"{name}\" status={status} services={services} running-tasks={running_tasks} pending-tasks={pending_tasks}"
        )
    }

    fn indent(&self) -> usize {
        0
    }
}

impl Present for Service {
    fn content(&self) -> String {
        let name = self.service_name().unwrap_or("unknown");
        let status = self.status().unwrap_or("unknown");
        let desired = self.desired_count();
        let running = self.running_count();
        let pending = self.pending_count();

        format!(
            "Service \"{name}\" status={status} desired={desired} running={running} pending={pending}"
        )
    }

    fn indent(&self) -> usize {
        2
    }
}

impl Present for Task {
    fn content(&self) -> String {
        // Extract task ID from ARN (last part after /)
        let task_id = self
            .task_arn()
            .and_then(|arn| arn.rsplit('/').next())
            .unwrap_or("unknown");
        let last_status = self.last_status().unwrap_or("unknown");
        let desired_status = self.desired_status().unwrap_or("unknown");
        let launch_type = self
            .launch_type()
            .map(|lt| lt.as_str())
            .unwrap_or("unknown");

        format!(
            "Task {task_id} status={last_status} desired={desired_status} launch-type={launch_type}"
        )
    }

    fn indent(&self) -> usize {
        4
    }
}

impl Present for ContainerInfo {
    fn content(&self) -> String {
        let status = self.last_status.as_deref().unwrap_or("unknown");
        let command_str = self
            .command
            .as_ref()
            .map(|cmd| format!(" command={:?}", cmd))
            .unwrap_or_default();

        format!(
            "Container \"{name}\" image={image} status={status}{command_str}",
            name = self.name,
            image = self.image,
        )
    }

    fn indent(&self) -> usize {
        6
    }
}

/// Let the user choose the cluster to use
async fn select_cluster(client: &aws_sdk_ecs::Client) -> eyre::Result<Option<String>> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    let client = client.clone();

    let fetch_handle = tokio::spawn(async move {
        let result: eyre::Result<()> = async {
            let mut paginator = client.list_clusters().into_paginator().send();

            while let Some(page) = paginator.next().await {
                let page = page.context("fetching clusters page")?;

                let cluster_arns: Vec<_> = page.cluster_arns().to_vec();
                if cluster_arns.is_empty() {
                    continue;
                }

                // Describe clusters to get names
                let clusters = client
                    .describe_clusters()
                    .set_clusters(Some(cluster_arns))
                    .send()
                    .await
                    .context("describing clusters")?;

                for cluster in clusters.clusters() {
                    let name = cluster.cluster_name().unwrap_or("unknown");
                    let arn = cluster.cluster_arn().unwrap_or("");
                    let status = cluster.status().unwrap_or("unknown");

                    let item = ClusterItem {
                        display: format!("{} ({})", name, status),
                        arn: arn.to_string(),
                    };

                    let _ = tx.send(Arc::new(item));
                }
            }

            Ok(())
        }
        .await;

        drop(tx);
        result
    });

    let options = SkimOptionsBuilder::default()
        .height("50%".to_string())
        .prompt("Select cluster: ".to_string())
        .build()
        .map_err(|e| eyre::eyre!("building skim options: {}", e))?;

    let selected = Skim::run_with(&options, Some(rx));

    let fetch_result = fetch_handle
        .await
        .context("background fetch task panicked")?;
    fetch_result?;

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

/// Let the user choose the service to use
async fn select_service(
    client: &aws_sdk_ecs::Client,
    cluster_arn: &str,
) -> eyre::Result<Option<String>> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();

    let client = client.clone();
    let cluster_arn = cluster_arn.to_string();

    let fetch_handle = tokio::spawn(async move {
        let result: eyre::Result<()> = async {
            let mut paginator = client
                .list_services()
                .cluster(&cluster_arn)
                .into_paginator()
                .send();

            while let Some(page) = paginator.next().await {
                let page = page.context("fetching services page")?;

                let service_arns: Vec<_> = page.service_arns().to_vec();
                if service_arns.is_empty() {
                    continue;
                }

                // Describe services to get names and status
                let services = client
                    .describe_services()
                    .cluster(&cluster_arn)
                    .set_services(Some(service_arns))
                    .send()
                    .await
                    .context("describing services")?;

                for service in services.services() {
                    let name = service.service_name().unwrap_or("unknown");
                    let arn = service.service_arn().unwrap_or("");
                    let status = service.status().unwrap_or("unknown");
                    let running = service.running_count();
                    let desired = service.desired_count();

                    let item = ServiceItem {
                        display: format!("{} ({}) {}/{}", name, status, running, desired),
                        arn: arn.to_string(),
                    };

                    let _ = tx.send(Arc::new(item));
                }
            }

            Ok(())
        }
        .await;

        drop(tx);
        result
    });

    let options = SkimOptionsBuilder::default()
        .height("50%".to_string())
        .prompt("Select service: ".to_string())
        .build()
        .map_err(|e| eyre::eyre!("building skim options: {}", e))?;

    let selected = Skim::run_with(&options, Some(rx));

    let fetch_result = fetch_handle
        .await
        .context("background fetch task panicked")?;
    fetch_result?;

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

/// Display an ECS service hierarchy
pub async fn display_ecs(
    config: &SdkConfig,
    cluster_arn: Option<String>,
    service_arn: Option<String>,
    writer: &dyn OutputWriter,
) -> eyre::Result<()> {
    let client = aws_sdk_ecs::Client::new(config);

    // Get or select cluster
    let cluster_arn = if let Some(arn) = cluster_arn {
        arn
    } else {
        match select_cluster(&client).await? {
            Some(arn) => arn,
            None => {
                eprintln!("No cluster selected");
                std::process::exit(1);
            }
        }
    };

    // Get cluster details
    let clusters = client
        .describe_clusters()
        .clusters(&cluster_arn)
        .send()
        .await
        .context("describing cluster")?;

    let cluster = clusters
        .clusters()
        .first()
        .ok_or_else(|| eyre::eyre!("Cluster not found: {}", cluster_arn))?;
    cluster.present(writer);

    // Get or select service
    let service_arn = if let Some(arn) = service_arn {
        arn
    } else {
        match select_service(&client, &cluster_arn).await? {
            Some(arn) => arn,
            None => {
                eprintln!("No service selected");
                std::process::exit(1);
            }
        }
    };

    // Get service details
    let services = client
        .describe_services()
        .cluster(&cluster_arn)
        .services(&service_arn)
        .send()
        .await
        .context("describing service")?;

    let service = services
        .services()
        .first()
        .ok_or_else(|| eyre::eyre!("Service not found: {}", service_arn))?;
    service.present(writer);

    // List tasks for this service
    let task_arns = client
        .list_tasks()
        .cluster(&cluster_arn)
        .service_name(service.service_name().unwrap_or(""))
        .send()
        .await
        .context("listing tasks")?;

    if task_arns.task_arns().is_empty() {
        return Ok(());
    }

    // Describe tasks
    let tasks = client
        .describe_tasks()
        .cluster(&cluster_arn)
        .set_tasks(Some(task_arns.task_arns().to_vec()))
        .send()
        .await
        .context("describing tasks")?;

    // Cache for task definitions to avoid redundant API calls
    let mut task_def_cache: HashMap<String, HashMap<String, ContainerInfo>> = HashMap::new();

    for task in tasks.tasks() {
        task.present(writer);

        // Get task definition to get container images
        if let Some(task_def_arn) = task.task_definition_arn() {
            let container_defs = if let Some(cached) = task_def_cache.get(task_def_arn) {
                cached.clone()
            } else {
                // Fetch task definition
                let task_def = client
                    .describe_task_definition()
                    .task_definition(task_def_arn)
                    .send()
                    .await
                    .context("describing task definition")?;

                let mut defs: HashMap<String, ContainerInfo> = HashMap::new();
                if let Some(td) = task_def.task_definition() {
                    for container_def in td.container_definitions() {
                        let name = container_def.name().unwrap_or("unknown").to_string();
                        let image = container_def.image().unwrap_or("unknown").to_string();
                        let command = {
                            let cmd = container_def.command();
                            if cmd.is_empty() {
                                None
                            } else {
                                Some(cmd.iter().map(|s| s.to_string()).collect())
                            }
                        };

                        defs.insert(
                            name.clone(),
                            ContainerInfo {
                                name,
                                image,
                                command,
                                last_status: None,
                            },
                        );
                    }
                }
                task_def_cache.insert(task_def_arn.to_string(), defs.clone());
                defs
            };

            // Get runtime container info and merge with definition
            for container in task.containers() {
                let container_name = container.name().unwrap_or("unknown");
                let last_status = container.last_status().map(|s| s.to_string());

                if let Some(mut info) = container_defs.get(container_name).cloned() {
                    info.last_status = last_status;
                    info.present(writer);
                } else {
                    // Container not in definition (shouldn't happen, but handle gracefully)
                    let info = ContainerInfo {
                        name: container_name.to_string(),
                        image: "unknown".to_string(),
                        command: None,
                        last_status,
                    };
                    info.present(writer);
                }
            }
        }
    }

    Ok(())
}
