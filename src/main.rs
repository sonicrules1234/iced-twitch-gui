use futures::TryStreamExt;
use home::home_dir;
use iced::alignment::Vertical::Top;
use iced::event::{self, Event};
use iced::widget::{button, column, container, image, row, scrollable, text};
use iced::{window, Center, Element, Fill, Renderer, Shrink, Subscription, Task};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
//use std::path::PathBuf;
use std::process::Stdio;
use twitch_api::helix::{streams::Stream, HelixClient};
use twitch_api::twitch_oauth2::{tokens, types::ClientId, AccessToken, Scope, UserToken};
use url::Url;
const CLIENT_ID: &str = "reh9rt391dkrperi4b7cqelryifsej";
#[derive(Clone)]
struct IcedTwitchGui {
    followed_streams: Vec<Stream>,
    client: HelixClient<'static, reqwest::Client>,
    token: Option<UserToken>,
    image_handles: Vec<image::Handle>,
    num_columns: usize,
}

#[derive(Clone, Debug)]
enum Message {
    Refresh,
    ClickedStream(usize),
    SaveRefresh((Vec<Stream>, Vec<image::Handle>)),
    Startup(String),
    GotUserToken(UserToken),
    GotChildProcessId(Option<u32>),
    OpenChat(usize),
    OpenChannel(usize),
    EventOccurred(Event),
}
async fn get_followed_streams(
    client: HelixClient<'static, reqwest::Client>,
    token: UserToken,
) -> Vec<Stream> {
    let followed_streams: Vec<twitch_api::helix::streams::Stream> = client
        .get_followed_streams(&token)
        .try_collect()
        .await
        .unwrap();
    followed_streams
}
async fn start_streaming(program: String, program_args: Vec<String>) -> Option<u32> {
    tokio::process::Command::new(program.as_str())
        .args(program_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
        .id()
}
async fn get_thumnails(streams: Vec<Stream>) -> Vec<image::Handle> {
    let mut image_handle_vec = Vec::new();
    for stream in streams {
        let req = reqwest::get(
            stream
                .thumbnail_url
                .replace("{width}", "320")
                .replace("{height}", "180"),
        )
        .await
        .unwrap();
        let data = req.bytes().await.unwrap();
        image_handle_vec.push(image::Handle::from_bytes(data));
    }
    image_handle_vec
}
async fn fetch_followed_streams_get_thumnails(
    client: HelixClient<'static, reqwest::Client>,
    user_token: UserToken,
) -> (Vec<Stream>, Vec<image::Handle>) {
    let streams = get_followed_streams(client.clone(), user_token.clone()).await;
    (streams.clone(), get_thumnails(streams.clone()).await)
}
async fn get_user_token(
    client: HelixClient<'static, reqwest::Client>,
    access_token_string: String,
) -> UserToken {
    let token = UserToken::from_token(&client, AccessToken::from(access_token_string.as_str()))
        .await
        .unwrap();
    token
}
impl IcedTwitchGui {
    fn new() -> Self {
        let client: HelixClient<reqwest::Client> = HelixClient::new();
        Self {
            followed_streams: Vec::new(),
            client: client,
            token: None,
            image_handles: Vec::new(),
            num_columns: 4,
        }
    }
    fn subscription(&self) -> Subscription<Message> {
        event::listen().map(Message::EventOccurred)
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::EventOccurred(evnt) => {
                if let Event::Window(window::Event::Resized(new_window_size)) = evnt {
                    self.num_columns = (new_window_size.width / 320.0).floor() as usize;
                }
                if let Event::Window(window::Event::Opened {
                    position: _position,
                    size,
                }) = evnt
                {
                    self.num_columns = (size.width / 320.0).floor() as usize;
                }
                Task::none()
            }
            Message::OpenChannel(idx) => {
                webbrowser::open(
                    format!(
                        "https://www.twitch.tv/{}",
                        self.followed_streams[idx].user_login
                    )
                    .as_str(),
                )
                .unwrap();
                Task::none()
            }
            Message::OpenChat(idx) => {
                webbrowser::open(
                    format!(
                        "https://www.twitch.tv/popout/{}/chat",
                        self.followed_streams[idx].user_login
                    )
                    .as_str(),
                )
                .unwrap();
                Task::none()
            }
            Message::GotChildProcessId(_pid_option) => Task::none(),
            Message::GotUserToken(user_token) => {
                self.token = Some(user_token);
                Task::perform(
                    fetch_followed_streams_get_thumnails(
                        self.client.clone(),
                        self.token.clone().unwrap(),
                    ),
                    Message::SaveRefresh,
                )
            }
            Message::Startup(access_token_string) => Task::perform(
                get_user_token(self.client.clone(), access_token_string),
                Message::GotUserToken,
            ),

            Message::Refresh => Task::perform(
                fetch_followed_streams_get_thumnails(
                    self.client.clone(),
                    self.token.clone().unwrap(),
                ),
                Message::SaveRefresh,
            ),
            Message::ClickedStream(idx) => {
                let this_stream = self.followed_streams[idx].clone();
                let this_username = this_stream.user_login.to_string();
                Task::perform(
                    start_streaming(
                        "twitch-hls-client".to_string(),
                        vec![this_username, "best".to_string()],
                    ),
                    Message::GotChildProcessId,
                )
            }
            Message::SaveRefresh((followed_streams, handle_vec)) => {
                self.followed_streams = followed_streams.clone();
                self.image_handles = handle_vec.clone();
                Task::none()
            }
        }
    }
    fn view(&self) -> Element<Message> {
        let mut this_grid: iced_aw::Grid<'static, Message, iced::Theme, Renderer> =
            iced_aw::Grid::new();
        let row_length = self.num_columns;
        let mut col_count = 0;
        let num_streams = self.followed_streams.clone().len();
        let mut this_grid_row: iced_aw::GridRow<'static, Message, iced::Theme, Renderer> =
            iced_aw::GridRow::new();
        for (i, stream) in self.followed_streams.clone().iter().enumerate() {
            let stream_title = stream.title.clone();
            if col_count < row_length {
                this_grid_row = this_grid_row.push(
                    container(column![
                        image::Image::new(self.image_handles[i].clone()),
                        row![
                            button("Play")
                                .width(Shrink)
                                .on_press(Message::ClickedStream(i)),
                            button("Chat").on_press(Message::OpenChat(i)),
                            button("Channel").on_press(Message::OpenChannel(i)),
                            text(format!("@{}", stream.user_login)).wrapping(text::Wrapping::None)
                        ],
                        text(stream.game_name.clone())
                            .wrapping(text::Wrapping::None)
                            .shaping(text::Shaping::Advanced),
                        text(stream_title)
                            .wrapping(text::Wrapping::None)
                            .shaping(text::Shaping::Advanced)
                    ])
                    .max_width(320)
                    .height(300)
                    .align_y(Top),
                );
                col_count += 1;
                if i + 1 == num_streams {
                    this_grid = this_grid.push(this_grid_row);
                    this_grid_row = iced_aw::GridRow::new();
                }
            } else {
                col_count = 1;
                this_grid = this_grid.push(this_grid_row);
                this_grid_row = iced_aw::GridRow::new();
                this_grid_row = this_grid_row.push(
                    container(column![
                        image::Image::new(self.image_handles[i].clone()),
                        row![
                            button("Play")
                                .width(Shrink)
                                .on_press(Message::ClickedStream(i)),
                            button("Chat").on_press(Message::OpenChat(i)),
                            button("Channel").on_press(Message::OpenChannel(i)),
                            text(format!("@{}", stream.user_login)).wrapping(text::Wrapping::None)
                        ],
                        text(stream.game_name.clone())
                            .wrapping(text::Wrapping::None)
                            .shaping(text::Shaping::Advanced),
                        text(stream_title)
                            .wrapping(text::Wrapping::None)
                            .shaping(text::Shaping::Advanced)
                    ])
                    .max_width(320)
                    .height(300)
                    .align_y(Top),
                );
                if i + 1 == num_streams {
                    this_grid = this_grid.push(this_grid_row);
                    this_grid_row = iced_aw::GridRow::new();
                }
            }
        }
        column![
            container(button("Refresh").on_press(Message::Refresh)).center_x(Fill),
            scrollable(this_grid).anchor_top().width(Fill)
        ]
        .align_x(Center)
        .into()
    }
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut p = home_dir().unwrap();
    for e in vec![".cache", "iced_twitch_gui"] {
        p.push(e);
        if !p.exists() {
            std::fs::create_dir(p.clone()).unwrap();
        }
    }
    if !home_dir()
        .unwrap()
        .join(".cache")
        .join("iced_twitch_gui")
        .join("access_token.txt")
        .exists()
    {
        let mut token_builder = tokens::ImplicitUserTokenBuilder::new(
            ClientId::from_static(CLIENT_ID),
            "http://localhost:5454/redirect".parse()?,
        );
        let access_token_string: String;
        token_builder = token_builder.set_scopes(vec![Scope::UserReadFollows]);
        webbrowser::open(token_builder.generate_url().0.as_str()).unwrap();
        let listener = TcpListener::bind("localhost:5454").unwrap();
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut reader = BufReader::new(&stream);

                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let redirect_url = request_line.split_whitespace().nth(1).unwrap();
                //println!("{redirect_url}");
                if redirect_url == "/redirect" {
                    let message = include_str!("../redirect.html");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        message.len(),
                        message
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    stream.flush().unwrap();
                    continue;
                } else {
                    let url =
                        Url::parse(&("http://localhost:5454".to_string() + redirect_url)).unwrap();
                    let code_pair = url
                        .query_pairs()
                        .find(|pair| {
                            let &(ref key, _) = pair;
                            key == "access_token"
                        })
                        .unwrap();

                    let (_, value) = code_pair;
                    access_token_string = value.to_string();
                    //println!("Access token: '{access_token_string}'");
                    let message = "You can now close this page";
                    let message_length = message.len();
                    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {message_length}\r\nContent-Type: text/plain\r\n\r\n{message}").as_bytes()).unwrap();
                    let mut f = std::fs::File::create(
                        home_dir()
                            .unwrap()
                            .join(".cache")
                            .join("iced_twitch_gui")
                            .join("access_token.txt"),
                    )
                    .unwrap();
                    f.write_all(access_token_string.as_bytes()).unwrap();
                    break;
                }
            }
        }
    }
    let mut window_settings = iced::window::Settings::default();
    window_settings.icon =
        Some(iced::window::icon::from_file_data(include_bytes!("../icon.png"), None).unwrap());

    iced::application(
        "Iced Twitch GUI",
        IcedTwitchGui::update,
        IcedTwitchGui::view,
    )
    .theme(|_| iced::theme::Theme::Dark)
    .window(window_settings)
    .subscription(IcedTwitchGui::subscription)
    .run_with(move || {
        let mut c = IcedTwitchGui::new();
        let d = c.update(Message::Startup(
            std::fs::read_to_string(
                home_dir()
                    .unwrap()
                    .join(".cache")
                    .join("iced_twitch_gui")
                    .join("access_token.txt"),
            )
            .unwrap(),
        ));
        (c, d)
    })
    .unwrap();
    Ok(())
}
