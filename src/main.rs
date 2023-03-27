use reqwest::header::{
	HeaderMap, HeaderName
};
use serde::Serialize;
use std::{
	env, collections::HashMap
};

use once_cell::sync::Lazy;
use serenity::async_trait;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{StandardFramework, CommandResult};

#[group]
#[commands(gpt, sys, clear, sysclear, token, memory)]
struct General;
struct Handler;

#[derive(Serialize)]
struct Req {
	model: String,
	messages: Vec<Msg>,
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

static mut _SYS: Lazy<HashMap<u64, Vec<Msg>>> = Lazy::new(|| {
	return HashMap::<u64, Vec<Msg>>::new();
});
static mut _MEM: Lazy<HashMap<u64, Vec<Mem>>> = Lazy::new(|| {
	return HashMap::<u64, Vec<Mem>>::new();
});

static mut _KEY: String = String::new();
static mut _MODEL: String = String::new();
static mut _MAX_TOKEN: i32 = 0;
static mut _PROMPT_LIMIT: i32 = 0;

#[async_trait]
impl EventHandler for Handler {
	async fn ready(&self, _ctx: Context, ready: Ready) {
		println!("{} is connected!", ready.user.name);
	}
}

#[tokio::main]
async fn main() {
	unsafe {
		_KEY = env::var("OPENAI_APIKEY").expect("Expected an API key in the environment, OPENAI_APIKEY");
		_MODEL = env::var("HAL_MODEL").unwrap_or("gpt-3.5-turbo".to_string());
		_MAX_TOKEN = env::var("HAL_MAX_TOKEN").unwrap_or("2560".to_string()).parse().unwrap();
		_PROMPT_LIMIT = env::var("HAL_PROMPT_LIMIT").unwrap_or("1536".to_string()).parse().unwrap();
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
		let sys = unsafe { _SYS.entry(guild_id).or_insert(Vec::new()) };
		let mem = unsafe { _MEM.entry(guild_id).or_insert(Vec::new()) };

		for s in sys.iter() {
			msgs.push(s.clone());
		}

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

		let response = client.post("https://api.openai.com/v1/chat/completions")
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
				String::from("ChatGPT API server didn't respond.")
			}
		}; // remove quotes by Value -> str -> String

		task.abort();
		msg.reply(ctx, context.clone()).await?;

		if save {
			let token = response["usage"]["total_tokens"].as_i64().unwrap() as i32 - calculate_token(guild_id); // TODO: calculate system message

			unsafe {
				if _MAX_TOKEN != 0 {
					let mut sum = _MAX_TOKEN - token;
					let mut index = 0;

					for (i, x) in mem.iter().enumerate().rev() {
						sum -= x.token;
						if sum <= 0 {
							index = i+1;
							break;
						}
					}

					(*mem).drain(..index);
				}

				(*mem).push(Mem {
					token: token,
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
async fn sys(ctx: &Context, msg: &Message) -> CommandResult {
	let guild_id = msg.guild_id.unwrap().0;
	let sys = unsafe { _SYS.entry(guild_id).or_insert(Vec::new()) };

	if msg.content.len() > 5 {
		(*sys).push(Msg {
			role: "system".to_owned(),
			name: "".to_owned(),
			content: msg.content[5..].to_string()
		});

		msg.reply(ctx, "System rule has been updated.").await?;
	} else if sys.len() == 0 {
		msg.reply(ctx, "No system rules.").await?;
	} else {
		let mut context = String::new();

		for (i, x) in sys.iter().enumerate() {
			context += format!("{}: {}\n", i+1, x.content).as_str();
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
async fn sysclear(ctx: &Context, msg: &Message) -> CommandResult {
	let guild_id = msg.guild_id.unwrap().0;
	let sys = unsafe { _SYS.entry(guild_id).or_insert(Vec::new()) };

	(*sys).clear();

	msg.reply(ctx, "System rule has been cleared.".to_string()).await?;
	Ok(())
}

#[command]
async fn token(ctx: &Context, msg: &Message) -> CommandResult {
	let guild_id = msg.guild_id.unwrap().0;
	let token = calculate_token(guild_id);

	unsafe {
		msg.reply(ctx, match _MAX_TOKEN {
			0 => format!("{} tokens used in memory.", token),
			_ => format!("{}/{} tokens used in memory.", token, _MAX_TOKEN)
		}).await?
	};

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

fn calculate_token(guild_id: u64) -> i32 {
	let mut token = 0;

	let mem = unsafe { _MEM.entry(guild_id).or_insert(Vec::new()) };

	for x in mem.iter() {
		token += x.token;
	}

	return token;
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