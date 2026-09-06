//! Give the native agent read-only access to application-owned inventory.
mod common;

use harnel::{
    Result, json,
    tool::{BoxFuture, Tool, ToolCall, ToolOutput, ToolSpec},
};
use std::collections::BTreeMap;

struct Inventory(BTreeMap<String, u64>);

impl Tool for Inventory {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "lookup_inventory".into(),
            description: "Read the available quantity of a product by SKU".into(),
            parameters: json!({"type":"object","properties":{"sku":{"type":"string"}},"required":["sku"],"additionalProperties":false}),
            read_only: true,
        }
    }

    fn execute(&self, call: ToolCall) -> BoxFuture<'_, Result<ToolOutput>> {
        Box::pin(async move {
            // Schemas guide the model; the application still validates input.
            let Some(sku) = call.arguments["sku"].as_str() else {
                return Ok(ToolOutput::failure("sku must be a string"));
            };
            let Some(quantity) = self.0.get(sku) else {
                return Ok(ToolOutput::failure("Unknown SKU"));
            };
            Ok(ToolOutput::text(
                json!({"sku":sku,"quantity":quantity}).to_string(),
            ))
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let inventory = Inventory(BTreeMap::from([
        ("keyboard".into(), 7),
        ("monitor".into(), 3),
    ]));
    let harness = common::builder()?.tool(inventory).build().await?;
    let result = async {
        let session = harness.session().await?;
        let answer = session
            .ask(common::prompt(
                "Use lookup_inventory to check keyboard stock.",
            ))
            .await?;
        println!("{}", answer.text);
        Ok(())
    }
    .await;
    let shutdown = harness.shutdown().await;
    result.and(shutdown)
}
