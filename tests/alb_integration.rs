mod common;

use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_elasticloadbalancingv2::Client as ElbClient;
use lbtree::present::BufferWriter;
use uuid::Uuid;

struct AlbTestFixture {
    config: aws_config::SdkConfig,
    ec2_client: Ec2Client,
    elb_client: ElbClient,
    vpc_id: Option<String>,
    subnet_ids: Vec<String>,
    security_group_id: Option<String>,
    lb_arn: Option<String>,
    target_group_arn: Option<String>,
    listener_arn: Option<String>,
}

impl AlbTestFixture {
    async fn new() -> color_eyre::Result<Self> {
        let config = common::localstack_config().await;
        let ec2_client = Ec2Client::new(&config);
        let elb_client = ElbClient::new(&config);

        let mut fixture = Self {
            config,
            ec2_client,
            elb_client,
            vpc_id: None,
            subnet_ids: Vec::new(),
            security_group_id: None,
            lb_arn: None,
            target_group_arn: None,
            listener_arn: None,
        };

        fixture.setup().await?;
        Ok(fixture)
    }

    async fn setup(&mut self) -> color_eyre::Result<()> {
        let test_id = Uuid::new_v4();
        let lb_name = format!("test-lb-{}", test_id);

        // Create VPC
        let vpc = self
            .ec2_client
            .create_vpc()
            .cidr_block("10.0.0.0/16")
            .send()
            .await?;
        self.vpc_id = vpc.vpc().and_then(|v| v.vpc_id().map(|s| s.to_string()));

        // Create subnets in different availability zones
        let subnet1 = self
            .ec2_client
            .create_subnet()
            .vpc_id(self.vpc_id.as_ref().unwrap())
            .cidr_block("10.0.1.0/24")
            .availability_zone("us-east-1a")
            .send()
            .await?;
        if let Some(id) = subnet1
            .subnet()
            .and_then(|s| s.subnet_id().map(|s| s.to_string()))
        {
            self.subnet_ids.push(id);
        }

        let subnet2 = self
            .ec2_client
            .create_subnet()
            .vpc_id(self.vpc_id.as_ref().unwrap())
            .cidr_block("10.0.2.0/24")
            .availability_zone("us-east-1b")
            .send()
            .await?;
        if let Some(id) = subnet2
            .subnet()
            .and_then(|s| s.subnet_id().map(|s| s.to_string()))
        {
            self.subnet_ids.push(id);
        }

        // Create security group
        let sg = self
            .ec2_client
            .create_security_group()
            .group_name(format!("test-sg-{}", test_id))
            .description("Test security group")
            .vpc_id(self.vpc_id.as_ref().unwrap())
            .send()
            .await?;
        self.security_group_id = sg.group_id().map(|s| s.to_string());

        // Create load balancer
        let lb = self
            .elb_client
            .create_load_balancer()
            .name(&lb_name)
            .set_subnets(Some(self.subnet_ids.clone()))
            .set_security_groups(self.security_group_id.as_ref().map(|sg| vec![sg.clone()]))
            .send()
            .await?;
        self.lb_arn = lb
            .load_balancers()
            .first()
            .and_then(|lb| lb.load_balancer_arn().map(|s| s.to_string()));

        // Create target group
        let tg = self
            .elb_client
            .create_target_group()
            .name(format!("test-tg-{}", test_id))
            .protocol(aws_sdk_elasticloadbalancingv2::types::ProtocolEnum::Http)
            .port(80)
            .vpc_id(self.vpc_id.as_ref().unwrap())
            .send()
            .await?;
        self.target_group_arn = tg
            .target_groups()
            .first()
            .and_then(|tg| tg.target_group_arn().map(|s| s.to_string()));

        // Create listener
        let listener = self
            .elb_client
            .create_listener()
            .load_balancer_arn(self.lb_arn.as_ref().unwrap())
            .protocol(aws_sdk_elasticloadbalancingv2::types::ProtocolEnum::Http)
            .port(80)
            .default_actions(
                aws_sdk_elasticloadbalancingv2::types::Action::builder()
                    .r#type(aws_sdk_elasticloadbalancingv2::types::ActionTypeEnum::Forward)
                    .target_group_arn(self.target_group_arn.as_ref().unwrap())
                    .build(),
            )
            .send()
            .await?;
        self.listener_arn = listener
            .listeners()
            .first()
            .and_then(|l| l.listener_arn().map(|s| s.to_string()));

        Ok(())
    }

    async fn run_display(&self) -> color_eyre::Result<String> {
        let writer = BufferWriter::new();
        lbtree::alb::display_alb(&self.config, self.lb_arn.clone(), &writer).await?;
        Ok(writer.get_output())
    }

    async fn cleanup(&mut self) -> color_eyre::Result<()> {
        // Clean up in reverse order of creation

        // Delete listener
        if let Some(arn) = &self.listener_arn {
            let _ = self
                .elb_client
                .delete_listener()
                .listener_arn(arn)
                .send()
                .await;
        }

        // Delete target group
        if let Some(arn) = &self.target_group_arn {
            let _ = self
                .elb_client
                .delete_target_group()
                .target_group_arn(arn)
                .send()
                .await;
        }

        // Delete load balancer
        if let Some(arn) = &self.lb_arn {
            let _ = self
                .elb_client
                .delete_load_balancer()
                .load_balancer_arn(arn)
                .send()
                .await;
        }

        // Delete security group
        if let Some(id) = &self.security_group_id {
            let _ = self
                .ec2_client
                .delete_security_group()
                .group_id(id)
                .send()
                .await;
        }

        // Delete subnets
        for subnet_id in &self.subnet_ids {
            let _ = self
                .ec2_client
                .delete_subnet()
                .subnet_id(subnet_id)
                .send()
                .await;
        }

        // Delete VPC
        if let Some(id) = &self.vpc_id {
            let _ = self.ec2_client.delete_vpc().vpc_id(id).send().await;
        }
        Ok(())
    }
}

impl Drop for AlbTestFixture {
    fn drop(&mut self) {
        // Use tokio runtime to run async cleanup
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.block_on(async {
                let _ = self.cleanup().await;
            });
        }
    }
}

#[tokio::test]
async fn test_alb_display() {
    skip_if_localstack_unavailable!();

    let fixture = AlbTestFixture::new()
        .await
        .expect("Failed to create test fixture");
    let output = fixture.run_display().await.expect("Failed to display ALB");

    dbg!(&output);

    // Verify output contains expected elements
    assert!(output.contains("Load balancer"));
    assert!(output.contains("Listener"));
    assert!(output.contains("Target group"));
}

#[tokio::test]
async fn test_alb_display_snapshot() {
    skip_if_localstack_unavailable!();

    let fixture = AlbTestFixture::new()
        .await
        .expect("Failed to create test fixture");
    let output = fixture.run_display().await.expect("Failed to display ALB");

    // Use insta for snapshot testing
    // Note: This will create a snapshot file that should be reviewed
    insta::assert_snapshot!(output);
}
