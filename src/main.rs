use blake2::{
  digest::{Update, VariableOutput},
  VarBlake2s,
};
use carapax::{
  handler,
  longpoll::LongPoll,
  methods::{
    AnswerCallbackQuery, AnswerInlineQuery, EditMessageText, SendMessage,
  },
  types::*,
  Api, Config, Dispatcher, ExecuteError, HandlerResult,
};
use dotenv::dotenv;
use env_logger::Target;
use log::{info, warn};
use regex::Regex;
use rustbreak::{deser::Yaml, PathDatabase};
use std::{collections::HashMap, env, fs, path::PathBuf};
use url::Url;

struct Context {
  api: Api,
  rhash_vec_db: PathDatabase<HashMap<String, Vec<String>>, Yaml>,
  default_setting_db: PathDatabase<HashMap<String, String>, Yaml>,
}

enum ReplyType {
  Message(String),
  DefaultRhash(Url, String),
  RhashVec(Url, Vec<String>),
}

#[tokio::main]
async fn main() {
  dotenv().ok();
  env_logger::init();

  let token = env::var("BOT_TOKEN").expect("BOT_TOKEN is not set");
  let proxy = env::var("BOT_PROXY").ok();
  let mut config = Config::new(token);
  if let Some(proxy) = proxy {
    config = config.proxy(proxy).expect("Failed to set proxy");
  }
  let api = Api::new(config).expect("Failed to create API");

  fs::create_dir_all("data").expect("Failed to create data directory");
  let rhash_vec_db =
    PathDatabase::<HashMap<String, Vec<String>>, Yaml>::load_from_path_or(
      PathBuf::from(r"data/rhash_vec_db.yaml"),
      HashMap::<String, Vec<String>>::new(),
    )
    .expect("Failed to initialize rhash_vec_db");
  let default_setting_db =
    PathDatabase::<HashMap<String, String>, Yaml>::load_from_path_or(
      PathBuf::from(r"data/default_setting_db.yaml"),
      HashMap::<String, String>::new(),
    )
    .expect("Failed to initialize default_setting_db");

  let mut dispatcher = Dispatcher::new(Context {
    api: api.clone(),
    rhash_vec_db,
    default_setting_db,
  });

  dispatcher.add_handler(handle_url);
  dispatcher.add_handler(handle_inline_query);
  dispatcher.add_handler(handle_callback_query);
  dispatcher.add_handler(handle_delete_command);

  LongPoll::new(api, dispatcher).run().await;
}

fn format_reply_iv_url(context: &Context, url: Url) -> Option<String> {
  let article_url = url
    .query_pairs()
    .find(|(k, _)| k == "url")
    .map(|(_, v)| v)?;
  let rhash = url
    .query_pairs()
    .find(|(k, _)| k == "rhash")
    .map(|(_, v)| v)?;
  let article_url = Url::parse(&article_url).ok()?;
  if let Some(host_str) = article_url.host_str() {
    if let Ok(rhash_vec) =
      context.rhash_vec_db.read(|db| db.get(host_str).cloned())
    {
      match rhash_vec {
        Some(mut rhash_vec) => {
          let rhash = rhash.into_owned();
          if !rhash_vec.contains(&rhash) {
            rhash_vec.push(rhash);
            if let Err(_) = context.rhash_vec_db.write(|db| {
              db.insert(host_str.to_owned(), rhash_vec.to_owned());
            }) {
              warn!("Unable to add data to the rhash_vec_db");
            }
          }
        }
        None => {
          if let Err(_) = context.rhash_vec_db.write(|db| {
            db.insert(host_str.to_owned(), vec![rhash.into_owned()]);
          }) {
            warn!("Unable to create data to the rhash_vec_db");
          }
        }
      }
      if let Err(_) = context.rhash_vec_db.save() {
        warn!("Unable to save data in the rhash_vec_db");
      }
    }
  }
  Some(format!(
    "[IV]({}) from [原文]({})",
    url.to_string(),
    article_url
  ))
}

fn get_user_and_url_hash(
  user: Option<&User>,
  url_host_str: &str,
) -> Option<String> {
  let mut hasher = VarBlake2s::new(10).ok()?;
  hasher.update(format!("{}{}", user?.id, url_host_str));
  Some(
    hasher
      .finalize_boxed()
      .iter()
      .map(|e| format!("{:0>2X}", e))
      .collect::<Vec<String>>()
      .join(""),
  )
}

fn reply_based_on_text(
  context: &Context,
  user: Option<&User>,
  text: &String,
) -> Option<ReplyType> {
  let url = Url::parse(text).ok()?;
  let url_host_str = url.host_str()?;
  if url_host_str == "t.me" && url.path() == "/iv" {
    Some(ReplyType::Message(format_reply_iv_url(context, url)?))
  } else {
    if let Some(hash_str) = get_user_and_url_hash(user, url_host_str) {
      let default_rhash = context
        .default_setting_db
        .read(|db| db.get(&hash_str).cloned());
      let rhash_vec = context
        .rhash_vec_db
        .read(|db| db.get(url_host_str).cloned());
      match (default_rhash, rhash_vec) {
        (Ok(Some(default_rhash)), _) => {
          Some(ReplyType::DefaultRhash(url, default_rhash))
        }
        (Ok(None), Ok(Some(rhash_vec))) => {
          Some(ReplyType::RhashVec(url, rhash_vec))
        }
        (_, _) => Some(ReplyType::Message(
          "抱歉，由于数据库错误或是没有相应的 rhash \
          所以无法为您生成 Instant View 链接"
            .to_owned(),
        )),
      }
    } else {
      Some(ReplyType::Message(
        "无法生成由用户生成的 hash 字符串".to_owned(),
      ))
    }
  }
}

#[handler]
async fn handle_url(
  context: &Context,
  message: Message,
) -> Result<HandlerResult, ExecuteError> {
  let reply = match message.get_text() {
    Some(text) => reply_based_on_text(context, message.get_user(), &text.data),
    None => None,
  };
  let method = match reply {
    Some(ReplyType::Message(reply)) => {
      SendMessage::new(message.get_chat_id(), reply)
        .parse_mode(ParseMode::MarkdownV2)
    }
    Some(ReplyType::DefaultRhash(url, rhash)) => SendMessage::new(
      message.get_chat_id(),
      format!(
        "[IV](https://t.me/iv?url={0}&rhash={1}) from [原文]({0})",
        url.to_string(),
        rhash
      ),
    )
    .parse_mode(ParseMode::MarkdownV2),
    Some(ReplyType::RhashVec(url, rhash_vec)) => {
      if let Some(rhash) = rhash_vec.first() {
        SendMessage::new(
          message.get_chat_id(),
          format!(
            "IV: https://t.me/iv?url={0}&rhash={1}\n原文: {0}\nrhash: {1}    (1/{2})",
            url.to_string(),
            rhash,
            rhash_vec.len()
          ),
        )
        .reply_markup(ReplyMarkup::InlineKeyboardMarkup(
          InlineKeyboardMarkup::from_vec(vec![vec![
            InlineKeyboardButton::new(
              "<",
              InlineKeyboardButtonKind::CallbackData("prev".to_owned()),
            ),
            InlineKeyboardButton::new(
              "选定",
              InlineKeyboardButtonKind::CallbackData("selected".to_owned()),
            ),
            InlineKeyboardButton::new(
              "设为默认",
              InlineKeyboardButtonKind::CallbackData("set as default".to_owned()),
            ),
            InlineKeyboardButton::new(
              ">",
              InlineKeyboardButtonKind::CallbackData("next".to_owned()),
            ),
          ]]),
        ))
      } else {
        SendMessage::new(
          message.get_chat_id(),
          "抱歉，由于数据库内没有相应的 rhash，所以无法为您生成 Instant View 链接",
        )
      }
    }
    None => SendMessage::new(
      message.get_chat_id(),
      "格式错误，请发送一个 URL，或检查您的 URL 是否正确".to_owned(),
    ),
  };
  context.api.execute(method).await?;
  Ok(HandlerResult::Stop)
}

#[handler]
async fn handle_inline_query(
  context: &Context,
  inline_query: InlineQuery,
) -> Result<HandlerResult, ExecuteError> {
  let method = match reply_based_on_text(
    context,
    Some(&inline_query.from),
    &inline_query.query,
  ) {
    Some(ReplyType::Message(reply)) => Some(AnswerInlineQuery::new(
      "inline_query.id",
      vec![InlineQueryResult::Article(InlineQueryResultArticle::new(
        "Result",
        "Result",
        InputMessageContent::Text(
          InputMessageContentText::new(&reply)
            .parse_mode(ParseMode::MarkdownV2),
        ),
      ))],
    )),
    Some(ReplyType::DefaultRhash(url, rhash)) => Some(
      AnswerInlineQuery::new(
        inline_query.id,
        vec![InlineQueryResult::Article(InlineQueryResultArticle::new(
          &rhash,
          &rhash,
          InputMessageContent::Text(
            InputMessageContentText::new(format!(
              "[IV](https://t.me/iv?url={0}&rhash={1}) from [原文]({0})",
              url.to_string(),
              &rhash
            ))
            .parse_mode(ParseMode::MarkdownV2),
          ),
        ))],
      )
      .cache_time(0),
    ),
    Some(ReplyType::RhashVec(url, rhash_vec)) => Some(
      AnswerInlineQuery::new(
        inline_query.id,
        rhash_vec
          .iter()
          .map(|rhash| {
            InlineQueryResult::Article(InlineQueryResultArticle::new(
              rhash,
              rhash,
              InputMessageContent::Text(
                InputMessageContentText::new(format!(
                  "[IV](https://t.me/iv?url={0}&rhash={1}) from [原文]({0})",
                  url.to_string(),
                  rhash
                ))
                .parse_mode(ParseMode::MarkdownV2),
              ),
            ))
          })
          .collect(),
      )
      .cache_time(0),
    ),
    None => None,
  };
  match method {
    Some(method) => {
      context.api.execute(method).await?;
      Ok(HandlerResult::Stop)
    }
    None => Ok(HandlerResult::Stop),
  }
}

#[handler]
async fn handle_callback_query(
  context: &Context,
  callback_query: CallbackQuery,
) -> Result<HandlerResult, ExecuteError> {
  if let Some(message) = callback_query.message {
    if let Some(text) = message.get_text() {
      let re = Regex::new(concat!(
        r"IV: (?P<iv_url>.+)\n",
        r"原文: (?P<article_url>https?://(?P<host_str>.+?)/.+)\n",
        r"rhash: (?P<rhash>\w{14})    \((?P<number>\d+)/\d+\)"
      ))
      .expect("正则表达式错误");
      let caps = re.captures(&text.data);
      if let Some(caps) = caps {
        if let (
          Some(iv_url),
          Some(article_url),
          Some(host_str),
          Some(rhash),
          Some(number),
        ) = (
          caps.name("iv_url"),
          caps.name("article_url"),
          caps.name("host_str"),
          caps.name("rhash"),
          caps.name("number"),
        ) {
          match callback_query.data.as_deref() {
            Some("selected") => {
              context
                .api
                .execute(
                  EditMessageText::new(
                    message.get_chat_id(),
                    message.id,
                    format!(
                      "[IV]({}) from [原文]({})",
                      iv_url.as_str(),
                      article_url.as_str()
                    ),
                  )
                  .parse_mode(ParseMode::MarkdownV2),
                )
                .await?;
              context
                .api
                .execute(AnswerCallbackQuery::new(callback_query.id))
                .await?;
            }
            Some("set as default") => {
              if let Some(hash_str) = get_user_and_url_hash(
                Some(&callback_query.from),
                host_str.as_str(),
              ) {
                if let Err(_) = context
                  .default_setting_db
                  .write(|db| db.insert(hash_str, rhash.as_str().to_owned()))
                {
                  warn!("Unable to insert data to the default_setting_db");
                }
                if let Err(_) = context.default_setting_db.save() {
                  warn!("Unable to save data in the default_setting_db");
                }
              }
              context
                .api
                .execute(
                  EditMessageText::new(
                    message.get_chat_id(),
                    message.id,
                    format!(
                      "[IV]({}) from [原文]({})",
                      iv_url.as_str(),
                      article_url.as_str()
                    ),
                  )
                  .parse_mode(ParseMode::MarkdownV2),
                )
                .await?;
              context
                .api
                .execute(AnswerCallbackQuery::new(callback_query.id))
                .await?;
            }
            Some(data @ "prev") | Some(data @ "next") => {
              if let Err(_) = context.rhash_vec_db.load() {
                warn!("Unable to load the latest data from the rhash_vec_db");
              }
              let rhash_vec = context
                .rhash_vec_db
                .read(|db| db.get(host_str.as_str()).cloned());
              if let (Ok(ordinal), Ok(Some(rhash_vec))) =
                (number.as_str().parse::<usize>(), rhash_vec)
              {
                match data {
                  "prev"
                    if rhash_vec.first()
                      == Some(&rhash.as_str().to_owned()) =>
                  {
                    context
                      .api
                      .execute(
                        AnswerCallbackQuery::new(callback_query.id)
                          .text("没有更靠前的模板"),
                      )
                      .await?;
                  }
                  "next"
                    if rhash_vec.last() == Some(&rhash.as_str().to_owned()) =>
                  {
                    context
                      .api
                      .execute(
                        AnswerCallbackQuery::new(callback_query.id)
                          .text("没有更靠后的模板"),
                      )
                      .await?;
                  }
                  "prev" if rhash_vec.len() >= 2 => {
                    if let Some(rhash) = rhash_vec.get(ordinal - 2) {
                      context
                      .api
                      .execute(
                        EditMessageText::new(
                          message.get_chat_id(),
                          message.id,
                          format!(
                            "IV: https://t.me/iv?url={0}&rhash={1}\n原文: {0}\nrhash: {1}    ({2}/{3})",
                            article_url.as_str(),
                            rhash,
                            ordinal - 1,
                            rhash_vec.len()
                          ),
                        )
                      )
                      .await?;
                      context
                        .api
                        .execute(AnswerCallbackQuery::new(callback_query.id))
                        .await?;
                    }
                  }
                  "next" if rhash_vec.len() >= 2 => {
                    if let Some(rhash) = rhash_vec.get(ordinal) {
                      context
                      .api
                      .execute(
                        EditMessageText::new(
                          message.get_chat_id(),
                          message.id,
                          format!(
                            "IV: https://t.me/iv?url={0}&rhash={1}\n原文: {0}\nrhash: {1}    ({2}/{3})",
                            article_url.as_str(),
                            rhash,
                            ordinal + 1,
                            rhash_vec.len()
                          ),
                        )
                      )
                      .await?;
                      context
                        .api
                        .execute(AnswerCallbackQuery::new(callback_query.id))
                        .await?;
                    }
                  }
                  _ => {}
                }
              }
            }
            Some(_) | None => {}
          }
        }
      }
    }
  }
  Ok(HandlerResult::Stop)
}

#[handler(command = "/deleteDefaultRhash")]
async fn handle_delete_command(
  context: &Context,
  command: Command,
) -> Result<HandlerResult, ExecuteError> {
  if let Some(url) = command.get_args().get(0) {
    if let Ok(url) = Url::parse(url) {
      if let Some(url_host_str) = url.host_str() {
        if let Some(hash_str) =
          get_user_and_url_hash(command.get_message().get_user(), url_host_str)
        {
          let result =
            context.default_setting_db.write(|db| db.remove(&hash_str));
          match result {
            Ok(Some(_)) => context.api.execute(SendMessage::new(
              command.get_message().get_chat_id(),
              "已删除对应的默认设置",
            )),
            Ok(None) => context.api.execute(SendMessage::new(
              command.get_message().get_chat_id(),
              "没找到对应的默认设置",
            )),
            Err(_) => {
              warn!("Unable to delete data from default_setting_db");
              context.api.execute(SendMessage::new(
                command.get_message().get_chat_id(),
                "删除默认 rhash 失败",
              ))
            }
          }
          .await?;
        }
      }
    }
  }
  Ok(HandlerResult::Stop)
}
