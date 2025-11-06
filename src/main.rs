use std::process::Stdio;

use aws_sdk_elasticloadbalancingv2::types::{
    Action, ActionTypeEnum, Listener, LoadBalancer, Rule, TargetGroup, TargetHealthDescription,
};
use clap::Parser;
use color_eyre::eyre::{self, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    load_balancer_arn: Option<String>,
}

trait Present: std::fmt::Debug {
    fn content(&self) -> String;

    fn indent(&self) -> usize;

    fn present(&self) {
        let prefix = " ".repeat(self.indent()) + "-> ";
        println!("{}{}", prefix, self.content());
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
                format!("Action (forward)")
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
async fn select_lbs(
    client: &aws_sdk_elasticloadbalancingv2::Client,
) -> eyre::Result<Option<String>> {
    let lb_arns: String = client
        .describe_load_balancers()
        .send()
        .await
        .context("describing load balancers")?
        .load_balancers()
        .into_iter()
        .map(|lb| lb.load_balancer_arn().unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    let mut proc = tokio::process::Command::new("fzf")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let mut stdin = proc.stdin.take().expect("failed to open stdin");
        stdin.write_all(lb_arns.as_bytes()).await?;
        drop(stdin);
    }

    let mut stdout = String::new();
    if let Some(mut out) = proc.stdout.take() {
        out.read_to_string(&mut stdout).await?;
    }

    let _status = proc.wait().await?;

    Ok(Some(stdout.trim().to_string()))
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let args = Args::parse();

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_elasticloadbalancingv2::Client::new(&config);

    let lb_arn = if let Some(arn) = args.load_balancer_arn {
        arn
    } else {
        select_lbs(&client)
            .await?
            .ok_or_else(|| eyre::eyre!("no lb selected"))?
    };
    // let lb_arn = args.load_balancer_arn.unwrap_or_else(|| {
    //     let selected_lb = select_lbs(client).await.unwrap()
    // });

    let load_balancer = client
        .describe_load_balancers()
        .load_balancer_arns(&lb_arn)
        .send()
        .await
        .context("describing load balancer")?;
    // panic safety: the client will return a 404 if the listener cannot be found, so we expect at
    // least one result
    let lb = &load_balancer.load_balancers()[0];
    lb.present();

    // listeners

    let listeners = client
        .describe_listeners()
        .load_balancer_arn(&lb_arn)
        .send()
        .await
        .wrap_err("describing listeners for load balancer")?;

    for listener in listeners.listeners() {
        listener.present();
        let listener_arn = if let Some(arn) = listener.listener_arn() {
            arn
        } else {
            continue;
        };

        // - rules
        let rules = client
            .describe_rules()
            .listener_arn(listener_arn)
            .send()
            .await
            .context("describing rules for listener")?;

        for rule in rules.rules() {
            rule.present();

            for action in rule.actions() {
                action.present();
            }
        }
    }

    // target groups
    let target_groups = client
        .describe_target_groups()
        .load_balancer_arn(&lb_arn)
        .send()
        .await
        .context("describing target groups")?;

    for target_group in target_groups.target_groups() {
        target_group.present();
        let tg_arn = if let Some(arn) = target_group.target_group_arn() {
            arn
        } else {
            continue;
        };

        // - targets
        let targets = client
            .describe_target_health()
            .target_group_arn(tg_arn)
            .send()
            .await
            .wrap_err("describing targets in target group")?;

        for target in targets.target_health_descriptions() {
            target.present();
        }
    }

    Ok(())
}
