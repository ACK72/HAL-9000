use reqwest::header::{
	HeaderMap, HeaderName
};
use serde::Serialize;
use serde_json::json;
use std::{
	env, collections::HashMap, io::stdin
};

use once_cell::sync::Lazy;
use serenity::async_trait;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{StandardFramework, CommandResult};

#[group]
#[commands(gpt, clear, token, memory, image)]
struct General;
struct Handler;

#[derive(Serialize)]
struct Req {
	model: String,
	messages: Vec<Msg>,
	#[serde(skip_serializing_if = "is_zero")]
	max_tokens: i32
}

#[derive(Clone, Serialize)]
struct Msg {
	role: String,
	#[serde(skip_serializing)]
	name: String,
	content: String
}

#[derive(Clone)]
struct Mem {
	token: i32,
	user: Msg,
	assistant: Msg
}

static mut _MEM: Lazy<HashMap<u64, Vec<Mem>>> = Lazy::new(|| {
	return HashMap::<u64, Vec<Mem>>::new();
});

static mut _KEY: String = String::new();
static mut _MODEL: String = String::new();

static mut _CHAT_ENDPOINT: String = String::new();
static mut _IMAGE_ENDPOINT: String = String::new();

static mut _DEBUG: bool = false;
static mut _MEMORY_LIMIT: i32 = 0;
static mut _PROMPT_LIMIT: i32 = 0;

#[async_trait]
impl EventHandler for Handler {
	async fn ready(&self, _ctx: Context, ready: Ready) {
		println!("{} is connected!", ready.user.name);
	}
}

#[tokio::main]
async fn main() {
	let _ = tokio::spawn(async move {
		loop {
			let mut buffer = String::new();
			stdin().read_line(&mut buffer).unwrap();
			
			let command = buffer.strip_suffix("\r\n").or(buffer.strip_suffix("\n")).unwrap();
			
			let message = match command {
				"debug true" => {
					unsafe { _DEBUG = true; }
					"Debug mode enabled."
				},
				"debug false" => {
					unsafe { _DEBUG = false; }
					"Debug mode disabled."
				},
				"help" => " - help\n - debug true/false",
				_ => "Unknown command. Use help to list possible command."
			};
			
			println!("{}", message);
		}
	});
	
	unsafe {
		_KEY = env::var("OPENAI_APIKEY").expect("Expected an API key in the environment, OPENAI_APIKEY");
		_MODEL = env::var("HAL_MODEL").unwrap_or("gpt-3.5-turbo".to_string());

		_CHAT_ENDPOINT = env::var("HAL_CHAT_ENDPOINT").unwrap_or("https://api.openai.com/v1/chat/completions".to_string());
		_IMAGE_ENDPOINT = env::var("HAL_IMAGE_ENDPOINT").unwrap_or("https://api.openai.com/v1/images/generations".to_string());
		
		_MEMORY_LIMIT = env::var("HAL_MEMORY_LIMIT").unwrap_or("2560".to_string()).parse().unwrap();
		_PROMPT_LIMIT = env::var("HAL_PROMPT_LIMIT").unwrap_or("0".to_string()).parse().unwrap();
	}
	let token = env::var("DISCORD_TOKEN").expect("Expected a Token in the environment, DISCORD_TOKEN");

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
		println!("An error occurred while running the client: {:?}", why);
	}
}

#[command]
async fn gpt(ctx: &Context, msg: &Message) -> CommandResult {
	if msg.content.len() > 5 {
		let http = ctx.http.clone();
		let channel_id = msg.channel_id.0;

		let task = tokio::spawn(async move {
			loop {
				let _ = http.broadcast_typing(channel_id).await;
			}
		});

		let client = reqwest::Client::new();
		let mut headers = HeaderMap::new();

		unsafe { headers.insert("Authorization".parse::<HeaderName>().unwrap(), format!("Bearer {}", _KEY).parse().unwrap()); }
		headers.insert("Content-Type".parse::<HeaderName>().unwrap(), "application/json".parse().unwrap());

		let mut msgs: Vec<Msg> = Vec::new();

		let guild_id = msg.guild_id.unwrap().0;
		let mem = unsafe { _MEM.entry(guild_id).or_insert(Vec::new()) };

		for m in mem.iter() {
			msgs.push(m.user.clone());
			msgs.push(m.assistant.clone());
		}

		let user = Msg {
			role: "user".to_owned(),
			name: msg.author.name.clone(),
			content: msg.content[5..].to_string()
		};

		msgs.push(user.clone());

		let req = Req {
			model: unsafe { _MODEL.clone() },
			messages: msgs,
			max_tokens: unsafe { _PROMPT_LIMIT }
		};

		let response = client.post(unsafe { _CHAT_ENDPOINT.clone() })
			.headers(headers)
			.body(serde_json::to_string(&req)?)
			.send()
			.await?
			.json::<serde_json::Value>()
			.await?;

		let mut save = true;
		let context = match response["choices"][0]["message"]["content"].as_str() {
			Some(v) => v.trim_matches(&['\r', '\n', ' '][..]).to_string(),
			None => {
				save = false;
				
				if unsafe { _DEBUG } {
					let raw = response.to_string();
					println!("{raw}");
				}
				
				String::from("ChatGPT API server didn't respond.")
			}
		}; // remove quotes by Value -> str -> String

		task.abort();
		long_message(ctx, msg, context.clone()).await;

		if save {
			let token = response["usage"]["total_tokens"].as_i64().unwrap() as i32 - calculate_token(guild_id);

			unsafe {
				if _MEMORY_LIMIT != 0 {
					let mut sum = _MEMORY_LIMIT - token;
					let mut index = 0;

					for (i, x) in mem.iter().enumerate().rev() {
						sum -= x.token;
						if sum < 0 {
							index = i+1;
							break;
						}
					}

					(*mem).drain(..index);
				}

				(*mem).push(Mem {
					token: token as i32,
					user: user.clone(),
					assistant: Msg {
						role: "assistant".to_owned(),
						name: "ChatGPT".to_owned(),
						content: context
					}
				});
			}
		}
	}

	Ok(())
}

#[command]
async fn memory(ctx: &Context, msg: &Message) -> CommandResult {
	let guild_id = msg.guild_id.unwrap().0;
	let mem = unsafe { _MEM.entry(guild_id).or_insert(Vec::new()) };

	if mem.len() == 0 {
		msg.reply(ctx, "No memories.".to_string()).await?;
	} else {
		let mut context = String::new();

		for x in mem.iter() {
			context += format!("{}: {}\n{}: {}\n", x.user.name, x.user.content, x.assistant.name, x.assistant.content).as_str();
		}

		long_message(ctx, msg, context).await;
	}

	Ok(())
}

#[command]
async fn clear(ctx: &Context, msg: &Message) -> CommandResult {
	let guild_id = msg.guild_id.unwrap().0;
	let mem = unsafe { _MEM.entry(guild_id).or_insert(Vec::new()) };

	(*mem).clear();

	msg.reply(ctx, "Memory has been cleared.").await?;
	
	Ok(())
}

#[command]
async fn token(ctx: &Context, msg: &Message) -> CommandResult {
	let guild_id = msg.guild_id.unwrap().0;
	let token = calculate_token(guild_id);

	unsafe {
		msg.reply(ctx, match _MEMORY_LIMIT {
			0 => format!("{} tokens used in memory.", token),
			_ => format!("{}/{} tokens used in memory.", token, _MEMORY_LIMIT)
		}).await?
	};

	Ok(())
}

#[command]
async fn image(ctx: &Context, msg: &Message) -> CommandResult {
	if msg.content.len() > 5 {
		let http = ctx.http.clone();
		let channel_id = msg.channel_id.0;

		let task = tokio::spawn(async move {
			loop {
				let _ = http.broadcast_typing(channel_id).await;
			}
		});

		let client = reqwest::Client::new();
		let mut headers = HeaderMap::new();

		unsafe { headers.insert("Authorization".parse::<HeaderName>().unwrap(), format!("Bearer {}", _KEY).parse().unwrap()); }
		headers.insert("Content-Type".parse::<HeaderName>().unwrap(), "application/json".parse().unwrap());

		let response = client.post(unsafe { _IMAGE_ENDPOINT.clone() })
			.headers(headers)
			.body(json!({
				"prompt": msg.content[5..].to_string(),
				"n": 1,
				"size": "1024x1024"
			}).to_string())
			.send()
			.await?
			.json::<serde_json::Value>()
			.await?;
		
		if unsafe { _DEBUG } {
			let raw = response.clone().to_string();
			println!("{raw}");
		}
		
		let context = match response["data"][0]["url"].as_str() {
			Some(v) => v.to_string(),
			None => {
				if unsafe { _DEBUG } {
					let raw = response.to_string();
					println!("{raw}");
				}
				String::from("DALL·E API server didn't respond.")
			}
		};

		task.abort();
		msg.reply(ctx, context.clone()).await?;
	}

	Ok(())
}

fn is_zero(n: &i32) -> bool {
	(*n) == 0
}

fn calculate_token(guild_id: u64) -> i32 {
	let mut token = 0;

	let mem = unsafe { _MEM.entry(guild_id).or_insert(Vec::new()) };

	for x in mem.iter() {
		token += x.token;
	}

	token
}

async fn long_message(ctx: &Context, msg: &Message, context: String) {
	let mut chars = context.chars();

	let contexts = (0..)
			.map(|_| chars.by_ref().take(2000).collect::<String>())
			.take_while(|s| !s.is_empty())
			.collect::<Vec<_>>();

	let mut reply = msg.clone();

	for string in contexts.iter() {
		reply = reply.reply(ctx, string).await.unwrap();
	}
}