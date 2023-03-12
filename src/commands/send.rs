use serenity::builder::CreateApplicationCommand;
use serenity::model::prelude::command::CommandOptionType;
use serenity::model::prelude::interaction::application_command::{
	CommandDataOption, CommandDataOptionValue
};
use reqwest::{
	Client, header::HeaderMap, header::HeaderName
};

pub async fn run(key: &String, options: &[CommandDataOption]) -> String {
	let role = match options.get(0).expect("Expected role").resolved.as_ref().unwrap() {
		CommandDataOptionValue::String(role) => role,
		_ => ""
	};
	let message = match options.get(0).expect("Expected message").resolved.as_ref().unwrap() {
		CommandDataOptionValue::String(message) => message,
		_ => ""
	};

	let client = Client::new();
	let mut headers = HeaderMap::new();

	headers.insert("Authorization".parse::<HeaderName>().unwrap(), format!("Bearer {}", key).parse().unwrap());
	headers.insert("Content-Type".parse::<HeaderName>().unwrap(), "application/json".parse().unwrap());

	let response = client.post("https://api.openai.com/v1/chat/completions")
        .headers(headers)
		.body(format!("{{\"model\": \"gpt-3.5-turbo\",\"messages\": [{{\"role\": \"{}\", \"content\": \"{}\"}}], \"max_tokens\": 1536}}", role, message))
        .send()
		.await
		.unwrap();

	response.json::<serde_json::Value>().await.unwrap()["choices"][0]["message"]["content"].as_str().unwrap().to_string() //remove quotes
}

pub fn register(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
	command
		.name("send")
		.description("Send to ChatGPT")
		.create_option(|option| {
			option
	            .name("role")
				.description("Role of Message")
	            .kind(CommandOptionType::String)
	            .required(true)
				.add_string_choice("system", "system")
				.add_string_choice("user", "user")
				.add_string_choice("assistant", "assistant")
		})
		.create_option(|option| {
			option
				.name("message")
				.description("Message to ChatGPT")
				.kind(CommandOptionType::String)
				.required(true)
		})
}