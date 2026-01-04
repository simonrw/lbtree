mod common;

use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_elasticloadbalancingv2::Client as ElbV2Client;
use aws_sdk_elasticloadbalancingv2::types::{
    ActionTypeEnum, FixedResponseActionConfig, ForwardActionConfig, LoadBalancerSchemeEnum,
    LoadBalancerTypeEnum, ProtocolEnum, RuleCondition, TargetGroupTuple, TargetTypeEnum,
};
use lbtree::present::BufferWriter;
use uuid::Uuid;

struct AlbTestFixture {
    config: aws_config::SdkConfig,
    elbv2_client: ElbV2Client,
    ec2_client: Ec2Client,

    // VPC resources (created first, deleted last)
    vpc_id: Option<String>,
    subnet_ids: Vec<String>,
    security_group_id: Option<String>,

    // ALB resources
    load_balancer_arn: Option<String>,
    listener_arn: Option<String>,
    target_group_arn: Option<String>,

    insta_settings: insta::Settings,
}

impl AlbTestFixture {
    async fn new() -> color_eyre::Result<Self> {
        let config = common::localstack_config().await;
        let elbv2_client = ElbV2Client::new(&config);
        let ec2_client = Ec2Client::new(&config);

        let mut fixture = Self {
            config,
            elbv2_client,
            ec2_client,
            vpc_id: None,
            subnet_ids: Vec::new(),
            security_group_id: None,
            load_balancer_arn: None,
            listener_arn: None,
            target_group_arn: None,
            insta_settings: insta::Settings::clone_current(),
        };

        fixture.setup().await?;
        Ok(fixture)
    }

    async fn setup(&mut self) -> color_eyre::Result<()> {
        let test_id = Uuid::new_v4();
        let short_id = &test_id.to_string()[..8];

        // 1. Create VPC
        let vpc = self
            .ec2_client
            .create_vpc()
            .cidr_block("10.0.0.0/16")
            .send()
            .await?;
        let vpc_id = vpc.vpc().unwrap().vpc_id().unwrap().to_string();
        self.vpc_id = Some(vpc_id.clone());

        // 2. Create subnets in different AZs (required for ALB)
        let subnet1 = self
            .ec2_client
            .create_subnet()
            .vpc_id(&vpc_id)
            .cidr_block("10.0.1.0/24")
            .availability_zone("us-east-1a")
            .send()
            .await?;
        let subnet1_id = subnet1.subnet().unwrap().subnet_id().unwrap().to_string();
        self.subnet_ids.push(subnet1_id);

        let subnet2 = self
            .ec2_client
            .create_subnet()
            .vpc_id(&vpc_id)
            .cidr_block("10.0.2.0/24")
            .availability_zone("us-east-1b")
            .send()
            .await?;
        let subnet2_id = subnet2.subnet().unwrap().subnet_id().unwrap().to_string();
        self.subnet_ids.push(subnet2_id);

        // 3. Create security group
        let sg_name = format!("test-sg-{}", short_id);
        let sg = self
            .ec2_client
            .create_security_group()
            .group_name(&sg_name)
            .description("Test security group for ALB")
            .vpc_id(&vpc_id)
            .send()
            .await?;
        let sg_id = sg.group_id().unwrap().to_string();
        self.security_group_id = Some(sg_id.clone());

        // 4. Create target group
        let tg_name = format!("test-tg-{}", short_id);
        self.insta_settings.add_filter(&tg_name, "[tg-name]");

        let tg = self
            .elbv2_client
            .create_target_group()
            .name(&tg_name)
            .protocol(ProtocolEnum::Http)
            .port(80)
            .vpc_id(&vpc_id)
            .target_type(TargetTypeEnum::Ip)
            .send()
            .await?;
        let tg_arn = tg
            .target_groups()
            .first()
            .unwrap()
            .target_group_arn()
            .unwrap()
            .to_string();
        self.target_group_arn = Some(tg_arn.clone());

        // 5. Create load balancer
        let lb_name = format!("test-lb-{}", short_id);

        let lb = self
            .elbv2_client
            .create_load_balancer()
            .name(&lb_name)
            .subnets(&self.subnet_ids[0])
            .subnets(&self.subnet_ids[1])
            .security_groups(&sg_id)
            .scheme(LoadBalancerSchemeEnum::Internal)
            .r#type(LoadBalancerTypeEnum::Application)
            .send()
            .await?;
        let lb_info = lb.load_balancers().first().unwrap();
        let lb_arn = lb_info.load_balancer_arn().unwrap().to_string();
        let lb_dns = lb_info.dns_name().unwrap().to_string();
        self.load_balancer_arn = Some(lb_arn.clone());
        self.insta_settings.add_filter(&lb_dns, "[lb-dns-name]");
        // make sure to filter the dns name first so that it gets completely replaced
        self.insta_settings.add_filter(&lb_name, "[lb-name]");

        // 6. Create listener with default forward action
        let listener = self
            .elbv2_client
            .create_listener()
            .load_balancer_arn(&lb_arn)
            .protocol(ProtocolEnum::Http)
            .port(80)
            .default_actions(
                aws_sdk_elasticloadbalancingv2::types::Action::builder()
                    .r#type(ActionTypeEnum::Forward)
                    .forward_config(
                        ForwardActionConfig::builder()
                            .target_groups(
                                TargetGroupTuple::builder()
                                    .target_group_arn(&tg_arn)
                                    .weight(1)
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await?;
        let listener_arn = listener
            .listeners()
            .first()
            .unwrap()
            .listener_arn()
            .unwrap()
            .to_string();
        self.listener_arn = Some(listener_arn.clone());

        // 7. Create non-default rule with fixed-response action
        let _ = self
            .elbv2_client
            .create_rule()
            .listener_arn(&listener_arn)
            .priority(100)
            .conditions(
                RuleCondition::builder()
                    .field("path-pattern")
                    .values("/api/*")
                    .build(),
            )
            .actions(
                aws_sdk_elasticloadbalancingv2::types::Action::builder()
                    .r#type(ActionTypeEnum::FixedResponse)
                    .fixed_response_config(
                        FixedResponseActionConfig::builder()
                            .status_code("200")
                            .content_type("text/plain")
                            .message_body("OK")
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await?;

        Ok(())
    }

    async fn run_display(&self) -> color_eyre::Result<String> {
        let writer = BufferWriter::new();
        lbtree::alb::display_alb(&self.config, self.load_balancer_arn.clone(), &writer).await?;
        Ok(writer.get_output())
    }

    async fn cleanup(&mut self) {
        // Delete in reverse order of creation

        // Delete listener (often deleted automatically with LB, but be explicit)
        if let Some(arn) = &self.listener_arn {
            let _ = self
                .elbv2_client
                .delete_listener()
                .listener_arn(arn)
                .send()
                .await;
        }

        // Delete load balancer
        if let Some(arn) = &self.load_balancer_arn {
            let _ = self
                .elbv2_client
                .delete_load_balancer()
                .load_balancer_arn(arn)
                .send()
                .await;
        }

        // Delete target group
        if let Some(arn) = &self.target_group_arn {
            let _ = self
                .elbv2_client
                .delete_target_group()
                .target_group_arn(arn)
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
    }
}

impl Drop for AlbTestFixture {
    fn drop(&mut self) {
        // Note: Explicit async cleanup in the test is preferred
        // This Drop impl serves as documentation that cleanup should be called
    }
}

#[tokio::test]
async fn test_alb_display_snapshot() {
    assert_localstack_available!();

    let mut fixture = AlbTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    // Ensure cleanup runs even if the test fails
    let result = async {
        let output = fixture.run_display().await?;
        Ok::<String, color_eyre::Report>(output)
    }
    .await;

    // Always cleanup, regardless of test success
    fixture.cleanup().await;

    // Now check the result
    let output = result.expect("error with test");

    // Use insta for snapshot testing
    fixture.insta_settings.bind(|| {
        insta::assert_snapshot!(output);
    });
}
