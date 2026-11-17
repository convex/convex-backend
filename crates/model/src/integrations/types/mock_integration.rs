use value::{
    obj,
    ConvexObject,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MockIntegrationConfig {}

impl TryFrom<ConvexObject> for MockIntegrationConfig {
    type Error = anyhow::Error;

    fn try_from(_value: ConvexObject) -> Result<Self, Self::Error> {
        Ok(MockIntegrationConfig {})
    }
}

impl TryFrom<MockIntegrationConfig> for ConvexObject {
    type Error = anyhow::Error;

    fn try_from(_value: MockIntegrationConfig) -> Result<Self, Self::Error> {
        obj!(
            "type" => "mock",
        )
    }
}
