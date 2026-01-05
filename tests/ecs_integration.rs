mod common;

use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_ecs::Client as EcsClient;
use aws_sdk_ecs::types::{
    AssignPublicIp, AwsVpcConfiguration, Compatibility, ContainerDefinition, KeyValuePair,
    NetworkConfiguration, NetworkMode,
};
use lbtree::present::BufferWriter;
use uuid::Uuid;

struct EcsTestFixture {
    config: aws_config::SdkConfig,
    ecs_client: EcsClient,
    ec2_client: Ec2Client,

    // VPC resources (for Fargate networking)
    vpc_id: Option<String>,
    subnet_id: Option<String>,
    security_group_id: Option<String>,

    // ECS resources
    cluster_arn: Option<String>,
    task_definition_arn: Option<String>,
    service_arn: Option<String>,

    insta_settings: insta::Settings,
}

impl EcsTestFixture {
    async fn new() -> color_eyre::Result<Self> {
        let config = common::localstack_config().await;
        let ecs_client = EcsClient::new(&config);
        let ec2_client = Ec2Client::new(&config);

        let mut fixture = Self {
            config,
            ecs_client,
            ec2_client,
            vpc_id: None,
            subnet_id: None,
            security_group_id: None,
            cluster_arn: None,
            task_definition_arn: None,
            service_arn: None,
            insta_settings: insta::Settings::clone_current(),
        };

        fixture.setup().await?;
        Ok(fixture)
    }

    async fn setup(&mut self) -> color_eyre::Result<()> {
        let test_id = Uuid::new_v4();
        let short_id = &test_id.to_string()[..8];

        // 1. Create VPC (required for Fargate)
        let vpc = self
            .ec2_client
            .create_vpc()
            .cidr_block("10.0.0.0/16")
            .send()
            .await?;
        let vpc_id = vpc.vpc().unwrap().vpc_id().unwrap().to_string();
        self.vpc_id = Some(vpc_id.clone());

        // 2. Create subnet
        let subnet = self
            .ec2_client
            .create_subnet()
            .vpc_id(&vpc_id)
            .cidr_block("10.0.1.0/24")
            .availability_zone("us-east-1a")
            .send()
            .await?;
        let subnet_id = subnet.subnet().unwrap().subnet_id().unwrap().to_string();
        self.subnet_id = Some(subnet_id.clone());

        // 3. Create security group
        let sg_name = format!("test-ecs-sg-{}", short_id);
        let sg = self
            .ec2_client
            .create_security_group()
            .group_name(&sg_name)
            .description("Test security group for ECS")
            .vpc_id(&vpc_id)
            .send()
            .await?;
        let sg_id = sg.group_id().unwrap().to_string();
        self.security_group_id = Some(sg_id.clone());

        // 4. Create ECS cluster
        let cluster_name = format!("test-cluster-{}", short_id);
        self.insta_settings
            .add_filter(&cluster_name, "[cluster-name]");

        let cluster = self
            .ecs_client
            .create_cluster()
            .cluster_name(&cluster_name)
            .send()
            .await?;
        let cluster_arn = cluster
            .cluster()
            .unwrap()
            .cluster_arn()
            .unwrap()
            .to_string();
        self.cluster_arn = Some(cluster_arn.clone());
        // Filter the cluster ARN (contains account ID and region)
        self.insta_settings
            .add_filter(&cluster_arn, "[cluster-arn]");

        // 5. Register task definition
        let task_family = format!("test-task-{}", short_id);
        self.insta_settings
            .add_filter(&task_family, "[task-family]");

        let task_def = self
            .ecs_client
            .register_task_definition()
            .family(&task_family)
            .network_mode(NetworkMode::Awsvpc)
            .requires_compatibilities(Compatibility::Fargate)
            .cpu("256")
            .memory("512")
            .container_definitions(
                ContainerDefinition::builder()
                    .name("app")
                    .image("nginx:latest")
                    .essential(true)
                    .cpu(128)
                    .memory(256)
                    .environment(KeyValuePair::builder().name("ENV").value("test").build())
                    .build(),
            )
            .container_definitions(
                ContainerDefinition::builder()
                    .name("sidecar")
                    .image("busybox:latest")
                    .essential(false)
                    .cpu(64)
                    .memory(128)
                    .command("echo")
                    .command("hello")
                    .build(),
            )
            .send()
            .await?;
        let task_def_arn = task_def
            .task_definition()
            .unwrap()
            .task_definition_arn()
            .unwrap()
            .to_string();
        self.task_definition_arn = Some(task_def_arn.clone());
        // Filter task definition ARN
        self.insta_settings
            .add_filter(&task_def_arn, "[task-definition-arn]");

        // 6. Create service
        let service_name = format!("test-service-{}", short_id);
        self.insta_settings
            .add_filter(&service_name, "[service-name]");

        let service = self
            .ecs_client
            .create_service()
            .cluster(&cluster_arn)
            .service_name(&service_name)
            .task_definition(&task_def_arn)
            .desired_count(1)
            .launch_type(aws_sdk_ecs::types::LaunchType::Fargate)
            .network_configuration(
                NetworkConfiguration::builder()
                    .awsvpc_configuration(
                        AwsVpcConfiguration::builder()
                            .subnets(&subnet_id)
                            .security_groups(&sg_id)
                            .assign_public_ip(AssignPublicIp::Disabled)
                            .build()?,
                    )
                    .build(),
            )
            .send()
            .await?;
        let service_arn = service
            .service()
            .unwrap()
            .service_arn()
            .unwrap()
            .to_string();
        self.service_arn = Some(service_arn.clone());
        // Filter service ARN
        self.insta_settings
            .add_filter(&service_arn, "[service-arn]");

        // Filter any task IDs that appear in the output (they're UUIDs)
        // We use a regex filter for task IDs which are the last part of task ARNs
        self.insta_settings.add_filter(
            r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            "[task-id]",
        );

        // Give LocalStack a moment to spin up tasks
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        Ok(())
    }

    async fn run_display(&self) -> color_eyre::Result<String> {
        let writer = BufferWriter::new();
        lbtree::ecs::display_ecs(
            &self.config,
            self.cluster_arn.clone(),
            self.service_arn.clone(),
            &writer,
        )
        .await?;
        Ok(writer.get_output())
    }

    async fn cleanup(&mut self) {
        // Delete in reverse order of creation

        // Update service to 0 tasks before deletion
        if let (Some(cluster_arn), Some(service_arn)) = (&self.cluster_arn, &self.service_arn) {
            let _ = self
                .ecs_client
                .update_service()
                .cluster(cluster_arn)
                .service(service_arn)
                .desired_count(0)
                .send()
                .await;

            // Delete service
            let _ = self
                .ecs_client
                .delete_service()
                .cluster(cluster_arn)
                .service(service_arn)
                .force(true)
                .send()
                .await;
        }

        // Deregister task definition
        if let Some(arn) = &self.task_definition_arn {
            let _ = self
                .ecs_client
                .deregister_task_definition()
                .task_definition(arn)
                .send()
                .await;
        }

        // Delete cluster
        if let Some(arn) = &self.cluster_arn {
            let _ = self.ecs_client.delete_cluster().cluster(arn).send().await;
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

        // Delete subnet
        if let Some(id) = &self.subnet_id {
            let _ = self.ec2_client.delete_subnet().subnet_id(id).send().await;
        }

        // Delete VPC
        if let Some(id) = &self.vpc_id {
            let _ = self.ec2_client.delete_vpc().vpc_id(id).send().await;
        }
    }
}

impl Drop for EcsTestFixture {
    fn drop(&mut self) {
        // Note: Explicit async cleanup in the test is preferred
        // This Drop impl serves as documentation that cleanup should be called
    }
}

#[tokio::test]
async fn test_ecs_display_snapshot() {
    assert_localstack_available!();

    let mut fixture = EcsTestFixture::new()
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
