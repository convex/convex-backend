//! A client for the Convex tutorial chat app.
//!
//! Please run this Convex Chat client from an initialized Convex project.
//! Check out the https://docs.convex.dev/get-started - to get started.
//!
//! Once you've initialized a Convex project with the tutorial, run this
//! demo from inside the project's working directory.
//!
//! For example:
//! cd /path/to/convex-rs
//! cargo build --example convex_chat_client
//! cd /path/to/convex-demos/tutorial
//! /path/to/convex-rs/target/debug/examples/convex_chat_client

use std::env;

use colored::Colorize;
use convex::{
    ConvexClient,
    FunctionResult,
    Value,
};
use futures::{
    pin_mut,
    select_biased,
    FutureExt,
    StreamExt,
};
use maplit::btreemap;
use tokio::sync::oneshot;

const SETUP_MSG: &str = r"
Please run this Convex Chat client from an initialized Convex project.
Check out the https://docs.convex.dev/get-started - to get started.

Once you've initialized a Convex project with the tutorial, run this
demo from inside the project's working directory.

For example:
cd /path/to/convex-rs
cargo build --example convex_chat_clientt
cd /path/to/convex-demos/tutorial
/path/to/convex-rs/target/debug/examples/convex_chat_client

";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load the tutorial's VITE_CONVEX_URL from the env file
    dotenvy::from_filename(".env.local").ok();
    dotenvy::dotenv().ok();
    let Ok(deployment_url) = env::var("VITE_CONVEX_URL") else {
        panic!("{SETUP_MSG}");
    };
    println!("Connecting to {deployment_url}");

    // Client code used in thread #1
    let mut client = ConvexClient::new(&deployment_url).await?;

    // Client code used in thread #2
    let mut client_ = client.clone();

    println!("{}", "Hi! What's your name?".to_string().red().bold());
    let mut sender = readline()?;
    if sender.is_empty() {
        sender = String::from("Anonymous Person");
    }

    let sender_clone = sender.clone();

    // Thread listening for new messages (use_query demo)
    let (cancel_sender, cancel_receiver) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let mut subscription = client
            .subscribe("messages:list", btreemap! {})
            .await
            .unwrap();

        let cancel_fut = cancel_receiver.fuse();
        pin_mut!(cancel_fut);
        loop {
            select_biased! {
                new_val = subscription.next().fuse() => {
                    let new_val = new_val.expect("Client dropped prematurely");
                    println!(
                        "{}",
                        "---------------- Message History ----------------".to_string().yellow()
                    );
                    if let FunctionResult::Value(Value::Array(array)) = new_val {
                        for item in array {
                            if let Value::Object(obj) = item {
                                if let Some(Value::String(str)) = obj.get("body") {
                                    let author = match obj.get("author") {
                                        Some(Value::String(name)) => name,
                                        _ => "Anonymous Author",
                                    };
                                    let author_string = if author == &sender_clone {
                                        author.to_string().yellow().bold()
                                    } else {
                                        author.to_string().red().bold()
                                    };
                                    println!("{author_string}: {str:?}");
                                }
                            }
                        }
                    }
                    println!(
                        "{}",
                        "-------------- End Message History --------------".to_string().yellow()
                    );
                },
                _ = cancel_fut => {
                    break
                },
            }
        }
        println!("Message listener closed");
    });

    // Loop for sending messages
    loop {
        let line = readline()?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line == "quit" || line == "exit" {
            println!(
                "{}",
                "------------- Exiting Convex Demo -------------"
                    .to_string()
                    .blue()
            );
            break;
        }

        println!("{}", "Sending a message".to_string().yellow().bold());
        let result = client_
            .mutation(
                "messages:send",
                btreemap! {
                    "body".to_string() => line.into(),
                    "author".to_string() => sender.clone().into(),
                },
            )
            .await?;
        match result {
            FunctionResult::Value(Value::Null) => {
                println!("{}.", "Message sent".to_string().green().bold());
            },
            FunctionResult::Value(v) => {
                println!(
                    "{}",
                    format!("Unexpected non-null result from messages:send {v:?}")
                        .red()
                        .bold()
                );
            },
            FunctionResult::ErrorMessage(err) => {
                println!("{}.", err.red().bold());
            },
            FunctionResult::ConvexError(err) => {
                println!("{err:?}");
            },
        };
    }

    cancel_sender
        .send(())
        .expect("Failed to send termination signal");
    handle.await?;

    Ok(())
}

fn readline() -> anyhow::Result<String> {
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer)?;
    if buffer.ends_with('\n') {
        buffer.pop();
        if buffer.ends_with('\r') {
            buffer.pop();
        }
    }
    Ok(buffer)
}
