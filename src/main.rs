mod commands;

use std::env;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{StandardFramework, CommandResult};
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::prelude::*;

use reqwest::{
	header::HeaderMap, header::HeaderName
};

#[group]
#[commands(gpt)]
struct General;
struct Handler;

static mut _KEY: String = String::new();

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
			let _ = command.create_interaction_response(&ctx.http, |response| {
				response.kind(InteractionResponseType::DeferredChannelMessageWithSource).interaction_response_data(|message| message)
			}).await;

			let content = match command.data.name.as_str() {
				"introduce" => commands::introduce::run(&command.data.options),
				"gpt" => unsafe { commands::gpt::run(&mut _KEY, &command.data.options).await },
				"send" => unsafe { commands::send::run(&mut _KEY, &command.data.options).await },
                _ => "not implemented :(".to_string(),
            };

			if let Err(why) = command.edit_original_interaction_response(&ctx.http, |response| {
				response.content(content)
			}).await{
				println!("Cannot respond to slash command: {}", why);
			}
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

		let guild_id = GuildId(env::var("GUILD_ID").expect("Expected GUILD_ID in environment").parse().expect("GUILD_ID must be an integer"));

		let _ = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands
                .create_application_command(|command| commands::introduce::register(command))
                .create_application_command(|command| commands::gpt::register(command))
                .create_application_command(|command| commands::send::register(command))
        })
        .await;
    }
}

#[tokio::main]
async fn main() {
	unsafe {
		_KEY = env::var("OPENAI_APIKEY").expect("Expected an API key in the environment, OPENAI_APIKEY");
	}

	let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

	let framework = StandardFramework::new()
        .configure(|c| c.prefix("`"))
        .group(&GENERAL_GROUP);
	let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;

	let mut client = Client::builder(token, intents)
        .event_handler(Handler)
		.framework(framework)
        .await
        .expect("Error creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
	}
}

#[command]
async fn gpt(ctx: &Context, msg: &Message) -> CommandResult {
	let _ = ctx.http.broadcast_typing(msg.channel_id.0).await;

	let client = reqwest::Client::new();
	let mut headers = HeaderMap::new();

	unsafe { headers.insert("Authorization".parse::<HeaderName>().unwrap(), format!("Bearer {}", _KEY).parse().unwrap()); }
	headers.insert("Content-Type".parse::<HeaderName>().unwrap(), "application/json".parse().unwrap());

	let response = client.post("https://api.openai.com/v1/chat/completions")
        .headers(headers)
		.body(format!("{{\"model\": \"gpt-3.5-turbo\", \"messages\": [{{\"role\": \"system\", \"content\": \"{}\"}}], \"max_tokens\": 1536}}", &msg.content[5..]))
		.send()
		.await
        .unwrap();

	let context = response.json::<serde_json::Value>().await.unwrap()["choices"][0]["message"]["content"].as_str().unwrap().to_string(); //remove quotes
	msg.reply(ctx, context).await?;

	Ok(())
}