use futures::TryStreamExt;
use iced::time::{self, Duration, Instant};
use home::home_dir;
use iced::alignment::Vertical::Top;
use iced::event::{self, Event};
use iced::widget::{button, column, container, image, row, scrollable, text, text_input, Space};
use iced::{
    window, Bottom, Center, Element, Fill, FillPortion, Padding, Renderer, Shrink, Subscription,
    Task,
};
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
    settings_open: bool,
    stream_command: String,
    player_command: String,
    twitch_oauth_token: String,
    stream_command_input: String,
    player_command_input: String,
    twitch_oauth_token_input: String,
    cache_path: std::path::PathBuf,
    currently_streaming_broadcasters: Vec<String>
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
    SettingsToggle,
    ApplySettings,
    PlayerCommandTextInputChanged(String),
    StreamCommandTextInputChanged(String),
    OAuthTokenTextInputChanged(String),
    OneMinute(Instant),
    CheckAndNotifyNewStreams(Vec<Stream>)
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
async fn start_streaming(stream_command: String, player_command: String, oauth_token: String, title: String, broadcaster_displayname: String, broadcaster_username: String) -> Option<u32> {
    let new_stream_command_parts: Vec<String> = serde_cmd::ArgIter::new(stream_command.as_str()).map(|x| x.replace("$title", &title).replace("$oauth_token", &oauth_token).replace("$broadcaster_username", &broadcaster_username).replace("$broadcaster_displayname", &broadcaster_displayname).replace("\"", "").to_string()).collect();
    let stream_program = new_stream_command_parts[0].clone();
    let stream_args = new_stream_command_parts[1..].to_vec();
    if player_command != String::new() {
        let new_player_command_parts: Vec<String> = serde_cmd::ArgIter::new(player_command.as_str()).map(|x| x.replace("$title", &title).replace("$oauth_token", &oauth_token).replace("$broadcaster_username", &broadcaster_username).replace("$broadcaster_displayname", &broadcaster_displayname).replace("\"", "").to_string()).collect();
        let player_program = new_player_command_parts[0].clone();
        let player_args = new_player_command_parts[1..].to_vec();
        let mut stream_cmd = tokio::process::Command::new(stream_program).args(stream_args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut stream_cmd_stdout = stream_cmd.stdout.take().unwrap();       
        let mut player_cmd = tokio::process::Command::new(player_program).args(player_args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut player_cmd_stdin = player_cmd.stdin.take().unwrap();
        tokio::io::copy(&mut stream_cmd_stdout, &mut player_cmd_stdin).await.unwrap();
        None
    } else {
        tokio::process::Command::new(stream_program).args(stream_args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap().id()
    }
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
        let settings_path = home_dir().unwrap().join(".cache").join("iced_twitch_gui");
        let stream_command = std::fs::read_to_string(settings_path.join("stream_command.txt"))
            .unwrap_or(String::from("twitch-hls-client $broadcaster_username"));
        let player_command = std::fs::read_to_string(settings_path.join("player_command.txt"))
            .unwrap_or(String::new());
        let oauth_token =
            std::fs::read_to_string(settings_path.join("oauth_token.txt")).unwrap_or(String::new());
        Self {
            followed_streams: Vec::new(),
            client: client,
            token: None,
            image_handles: Vec::new(),
            num_columns: 4,
            settings_open: false,
            stream_command: stream_command.clone(),
            player_command: player_command.clone(),
            twitch_oauth_token: oauth_token.clone(),
            stream_command_input: stream_command.clone(),
            player_command_input: player_command.clone(),
            twitch_oauth_token_input: oauth_token.clone(),
            cache_path: home_dir().unwrap().join(".cache").join("iced_twitch_gui"),
            currently_streaming_broadcasters: Vec::new()
        }
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
        event::listen().map(Message::EventOccurred),
            time::every(Duration::from_secs(60)).map(Message::OneMinute)
        ])
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CheckAndNotifyNewStreams(streams) => {
                let mut new_broadcasters: Vec<String> = Vec::new();
                let mut new_current_broadcasters: Vec<String> = Vec::new();
                for stream in streams {
                    let this_broadcaster = stream.user_login.to_string();
                    if !self.currently_streaming_broadcasters.contains(&this_broadcaster) {
                        new_broadcasters.push(this_broadcaster.clone());
                    }
                    new_current_broadcasters.push(this_broadcaster.clone());
                }
                self.currently_streaming_broadcasters = new_current_broadcasters.clone();
                if new_broadcasters.len() > 0 {
                    let notif_message = format!("The following streamers have started streaming: {}", new_broadcasters.join(", "));
                    notify_rust::Notification::new().summary("Iced Twitch GUI").body(notif_message.as_str()).show().unwrap();
                }
                Task::none()

            }
            Message::OneMinute(_instant) => {
                Task::perform(get_followed_streams(self.client.clone(), self.token.clone().unwrap()), Message::CheckAndNotifyNewStreams)
            }
            Message::StreamCommandTextInputChanged(new_si) => {
                self.stream_command_input = new_si.clone();
                Task::none()
            }
            Message::PlayerCommandTextInputChanged(new_pi) => {
                self.player_command_input = new_pi.clone();
                Task::none()
            }
            Message::OAuthTokenTextInputChanged(new_oi) => {
                self.twitch_oauth_token_input = new_oi.clone();
                Task::none()
            }
            Message::SettingsToggle => {
                self.settings_open = !self.settings_open;
                Task::none()
            }
            Message::ApplySettings => {
                self.stream_command = self.stream_command_input.clone();
                self.player_command = self.player_command_input.clone();
                self.twitch_oauth_token = self.twitch_oauth_token_input.clone();
                {
                    let mut f =
                        std::fs::File::create(self.cache_path.join("stream_command.txt")).unwrap();
                    f.write_all(self.stream_command.as_bytes()).unwrap();
                }
                {
                    let mut f =
                        std::fs::File::create(self.cache_path.join("player_command.txt")).unwrap();
                    f.write_all(self.player_command.as_bytes()).unwrap();
                }
                {
                    let mut f =
                        std::fs::File::create(self.cache_path.join("oauth_token.txt")).unwrap();
                    f.write_all(self.twitch_oauth_token.as_bytes()).unwrap();
                }
                self.settings_open = false;
                Task::none()
            }
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
                let broadcaster_username = this_stream.user_login.to_string();
                let broadcaster_displayname = this_stream.user_name.to_string();
                let title = this_stream.title.to_string();
                Task::perform(
                    start_streaming(self.stream_command.clone(), self.player_command.clone(), self.twitch_oauth_token.clone(), title.clone(), broadcaster_displayname.clone(), broadcaster_username.clone()
                    ),
                    Message::GotChildProcessId,
                )
            }
            Message::SaveRefresh((followed_streams, handle_vec)) => {
                self.followed_streams = followed_streams.clone();
                self.image_handles = handle_vec.clone();
                self.currently_streaming_broadcasters = self.followed_streams.clone().iter().map(|x| x.user_login.to_string()).collect();
                Task::none()
            }
        }
    }
    fn view(&self) -> Element<Message> {
        if self.settings_open {
            column![
                Space::with_height(10), container(text("If both stream command an player command are filled, the stdout of the stream command will be piped to the player command.  If the stream command is filled out and the player command isn't, then only the stream command is run.  $title, $broadcaster_displayname, $broadcaster_username, and $oauth_token will be replaced with their respective values.  Escaped quotes may cause problems.")).center_x(Fill).padding(10), 
                row![container(text("Stream command: ")).align_right(Fill).width(FillPortion(1)), container(text_input("Put your stream command here...", self.stream_command_input.as_str()).on_input(Message::StreamCommandTextInputChanged)).align_left(Fill).width(FillPortion(2)).padding(Padding::from([0, 10]))],
                row![container(text("Player command: ")).align_right(Fill).width(FillPortion(1)), container(text_input("Put your player command here...", self.player_command_input.as_str()).on_input(Message::PlayerCommandTextInputChanged)).align_left(Fill).width(FillPortion(2)).padding(Padding::from([0, 10]))],
                row![container(text("OAuth Token: ")).align_right(Fill).width(FillPortion(1)), container(text_input("Put the twitch oauth token from your browser here...", self.twitch_oauth_token_input.as_str()).on_input(Message::OAuthTokenTextInputChanged)).align_left(Fill).width(FillPortion(2)).padding(Padding::from([0, 10]))],
                Space::with_height(Fill),
                row![
                container(button("Cancel").on_press(Message::SettingsToggle)).center_x(Fill),
                container(button("Apply").on_press(Message::ApplySettings)).center_x(Fill)
            ].align_y(Bottom) 
            ]
            .into()
        } else {
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
                                text(format!("@{}", stream.user_login))
                                    .wrapping(text::Wrapping::None)
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
                                text(format!("@{}", stream.user_login))
                                    .wrapping(text::Wrapping::None)
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
                row![
                    container(button("Settings").on_press(Message::SettingsToggle)).center_x(Fill),
                    container(button("Refresh").on_press(Message::Refresh)).center_x(Fill)
                ],
                scrollable(this_grid).anchor_top().width(Fill)
            ]
            .align_x(Center)
            .into()
        }
    }
}
fn main() -> Result<(), iced::Error> {
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
            "http://localhost:5454/redirect".parse().unwrap(),
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
}
