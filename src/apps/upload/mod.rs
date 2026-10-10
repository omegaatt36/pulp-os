// wifi upload server: TCP accept + mDNS (pulp.local); the HTTP layer is http.rs,
// the mDNS responder mdns.rs

mod connect;
mod http;
mod mdns;
mod session;

use core::alloc::Layout;
use core::fmt::Write as FmtWrite;
use core::mem::{align_of, size_of};

use embassy_futures::select::{Either, select, select3};
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, RecvError, UdpSocket};
use embassy_net::{
    IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address, Runner, Stack, StackResources,
};
use embassy_time::{Duration, Timer};
use esp_hal::delay::Delay;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config, ControllerConfig, Interface, Password, Ssid, WifiController,
};
use log::info;
use pulp_board_logic::upload::{self, NetProfile};

use self::connect::ConnectError;
use self::http::ServerEvent;
use self::session::SessionEnd;
use crate::board::action::{Action, ActionEvent, ButtonMapper};
use crate::board::{self, Epd, SCREEN_H, SCREEN_W};
use crate::drivers::sdcard::SdStorage;
use crate::drivers::strip::StripBuffer;
use crate::fonts;
use crate::fonts::bitmap::BitmapFont;
use crate::kernel::bigbuf::{BufClass, DecoderScratch};
use crate::kernel::config::WifiConfig;
use crate::kernel::tasks;
use crate::ui::{
    Alignment, BitmapLabel, ButtonFeedback, CONTENT_TOP, LARGE_MARGIN, QrSymbol, Region, stack_fmt,
};

const HEADING_X: u16 = LARGE_MARGIN;
const HEADING_W: u16 = SCREEN_W - HEADING_X * 2;

const BODY_X: u16 = 24;
const BODY_W: u16 = SCREEN_W - BODY_X * 2;
const BODY_LINE_GAP: u16 = 10;
const FOOTER_Y: u16 = SCREEN_H - 60;

// gap between the connection lines and the QR code, and the largest square
// the code may take (a version 2 symbol then draws at 8 px per module)
const QR_GAP: u16 = 24;
const QR_MAX_SIDE: u16 = 280;

// HTTP timing
const HTTP_TIMEOUT_SECS: u64 = 30;
const ACCEPT_RETRY_MS: u64 = 200;
const DHCP_POLL_MS: u64 = 100;

const SOCKET_CLOSE_DELAY_MS: u64 = 50;

// Everything one session needs besides the radio and the network stack, in one
// block: the HTTP request state (`http::HttpScratch`) followed by the TCP
// receive, TCP transmit and HTTP work buffers, sized by a `NetProfile`
// (pulp_board_logic::upload). Only tasks touch it (never an ISR or DMA), so
// the C61 keeps it in PSRAM instead of in the executor's static arena.
struct Scratch<'a> {
    tcp_rx: &'a mut [u8],
    tcp_tx: &'a mut [u8],
    work: &'a mut [u8],
    request: &'a mut http::HttpScratch,
}

// One zeroed block on the board's scratch placement, taken before the radio
// starts so that running out of memory ends the session cleanly. The HR8
// profile is tried first; if it is refused the small one (the X4 sizes) is
// tried, and only when that fails too does the session end.
struct ScratchBlock {
    block: DecoderScratch,
    profile: NetProfile,
}

impl ScratchBlock {
    fn layout(profile: NetProfile) -> Layout {
        // `HttpScratch`'s size is a multiple of its alignment, so the byte
        // buffers after it need no padding
        Layout::from_size_align(
            size_of::<http::HttpScratch>() + profile.total(),
            align_of::<http::HttpScratch>(),
        )
        .expect("scratch layout is valid")
    }

    fn new() -> Result<Self, ConnectError> {
        #[cfg(feature = "board-onepage-c61")]
        let status = pulp_kernel::board_c61::memory::status();
        #[cfg(not(feature = "board-onepage-c61"))]
        let status = pulp_board_logic::memory::PsramStatus::NotInitialised;

        let (profile, block) = upload::acquire(status, |profile| {
            DecoderScratch::zeroed(BufClass::NetScratch, Self::layout(profile)).inspect_err(|_| {
                info!(
                    "upload: no memory for the {} KiB session scratch",
                    profile.total() / 1024
                )
            })
        })
        .map_err(|_| ConnectError::OutOfMemory)?;
        info!(
            "upload: session scratch rx {} tx {} work {} bytes",
            profile.rx, profile.tx, profile.work
        );
        Ok(Self { block, profile })
    }

    fn get(&mut self) -> Scratch<'_> {
        let base = self.block.ptr();
        let NetProfile { rx, tx, work } = self.profile;
        // SAFETY: the block is `layout(profile).size()` bytes at the alignment
        // of `HttpScratch`, zero-initialised and owned exclusively through
        // `&mut self`; all-zero is a valid `HttpScratch` (byte arrays, and
        // `DirEntry` whose fields are integers and a bool). The four regions
        // are disjoint, in bounds, and byte slices need no alignment.
        unsafe {
            let bytes = base.add(size_of::<http::HttpScratch>());
            Scratch {
                request: &mut *base.cast::<http::HttpScratch>(),
                tcp_rx: core::slice::from_raw_parts_mut(bytes, rx),
                tcp_tx: core::slice::from_raw_parts_mut(bytes.add(rx), tx),
                work: core::slice::from_raw_parts_mut(bytes.add(rx + tx), work),
            }
        }
    }
}

// The radio and network context of one session. `session::run` creates it and
// drops it before returning. Field order is the drop order: the runner owns the
// station `Interface` (dropping it releases the interface singleton) and goes
// first, then the stack handle, and the controller last (its drop deinitialises
// the Wi-Fi driver), so the network never outlives the radio; the scratch is
// last, nothing in front of it refers to it. The
// `StackResources` live in `run_upload_mode`, outside `session::run`, and only
// hold socket storage.
struct Net<'a> {
    runner: Runner<'a, Interface>,
    stack: Stack<'a>,
    controller: WifiController<'static>,
    scratch: ScratchBlock,
}

pub async fn run_upload_mode(
    epd: &mut Epd,
    strip: &mut StripBuffer,
    delay: &mut Delay,
    sd: &SdStorage,
    ui_font_size_idx: u8,
    bumps: &ButtonFeedback,
    wifi_cfg: &WifiConfig,
) {
    let heading = fonts::heading_font(ui_font_size_idx);
    let body = fonts::chrome_font();

    let ssid = wifi_cfg.ssid();
    let password = wifi_cfg.password();

    // `session::run` rejects bad credentials itself; this only keeps the
    // "Connecting" screen off a session that fails immediately.
    if connect::check_credentials(ssid.as_bytes(), password.as_bytes()).is_ok() {
        let mut msg_buf = [0u8; 64];
        let msg_len = stack_fmt(&mut msg_buf, |w| {
            let _ = write!(w, "Connecting to '{}'...", ssid);
        });
        let msg = core::str::from_utf8(&msg_buf[..msg_len]).unwrap_or("Connecting...");
        render_screen(
            epd,
            strip,
            delay,
            heading,
            body,
            &[msg],
            None,
            None,
            bumps,
            true,
        )
        .await;
    }

    let net_config = embassy_net::Config::dhcpv4(Default::default());
    let seed = {
        let rng = esp_hal::rng::Rng::new();
        (rng.random() as u64) << 32 | rng.random() as u64
    };
    let mut resources = StackResources::<4>::new();
    let resources = &mut resources;

    let end = session::run(
        ssid.as_bytes(),
        password.as_bytes(),
        connect::Limits::DEFAULT,
        move || {
            // The interface singleton is taken first: a stale owner yields
            // `None` here, before the radio is touched, and ends the session
            // with an error instead of a panic.
            let interface = Interface::try_station().ok_or_else(|| {
                info!("upload: station interface already taken");
                ConnectError::RadioUnavailable
            })?;
            let station_cfg = station_config(ssid, password)?;
            let scratch = ScratchBlock::new()?;
            // Safety: WIFI has no other user, and only the holder of the
            // station interface singleton (taken above) gets here, so there is
            // no second controller; it is not used again once `Net` is dropped.
            let wifi = unsafe { esp_hal::peripherals::WIFI::steal() };
            let controller = WifiController::new(
                wifi,
                ControllerConfig::default().with_initial_config(Config::Station(station_cfg)),
            )
            .map_err(|e| {
                info!("upload: wifi init failed: {:?}", e);
                ConnectError::RadioUnavailable
            })?;
            let (stack, runner) = embassy_net::new(interface, net_config, resources, seed);
            Ok(Net {
                runner,
                stack,
                controller,
                scratch,
            })
        },
        async |net: &mut Net<'_>| {
            info!("upload: wifi initialised, connecting to '{}'", ssid);
            net.controller
                .connect_async()
                .await
                .map(drop)
                .map_err(|e| info!("upload: connect failed: {:?}", e))
        },
        async |net: &mut Net<'_>| {
            info!("upload: connected to '{}', waiting for DHCP", ssid);
            let Net { runner, stack, .. } = net;
            // Only an IPv4 address ends the stage; `session::run` bounds the
            // wait with the DHCP limit, so a link that never gets one ends as
            // `DhcpTimeout` rather than serving (and showing) 0.0.0.0.
            let ipv4 = async {
                loop {
                    stack.wait_config_up().await;
                    if let Some(cfg) = stack.config_v4() {
                        break cfg.address.address().octets();
                    }
                    Timer::after(Duration::from_millis(DHCP_POLL_MS)).await;
                }
            };
            match select(runner.run(), ipv4).await {
                Either::First(never) => match never {},
                Either::Second(octets) => octets,
            }
        },
        async |net: &mut Net<'_>, ip_octets: [u8; 4]| {
            let mut ip_buf = [0u8; mdns::IP_LABEL_MAX];
            let ip_str = mdns::ip_label(ip_octets, &mut ip_buf);

            info!("upload: serving at http://pulp.local/  {}", ip_str);

            // the QR carries the IP URL: mDNS is unreliable on phones
            let [a, b, c, d] = ip_octets;
            let mut url_buf = [0u8; 32];
            let url_len = stack_fmt(&mut url_buf, |w| {
                let _ = write!(w, "http://{}.{}.{}.{}/", a, b, c, d);
            });
            let url = core::str::from_utf8(&url_buf[..url_len]).unwrap_or("");
            let qr = QrSymbol::encode(url);

            render_screen(
                epd,
                strip,
                delay,
                heading,
                body,
                &["http://pulp.local/", ip_str],
                Some("Press BACK to exit"),
                qr.as_ref(),
                bumps,
                false,
            )
            .await;

            // The network runner, the HTTP server and the mDNS responder run
            // for the whole session; BACK (raced by `session::run`) ends it.
            let Net {
                runner,
                stack,
                scratch,
                ..
            } = net;
            let never = select3(
                runner.run(),
                serve_http(*stack, scratch.get(), sd),
                serve_mdns(*stack, ip_octets),
            )
            .await;
            match never {}
        },
        drain_until_back(),
    )
    .await;

    // The session is over: the stage futures, the stack, the interface and the
    // radio controller are all released, so the screens below run without them.
    match end {
        SessionEnd::Exited(phase) => info!("upload: user exited ({:?}), WiFi released", phase),
        SessionEnd::Failed(e) => {
            info!("upload: failed: {:?}, WiFi released", e);
            show_error(epd, strip, delay, heading, body, e, bumps).await;
        }
    }
}

async fn serve_http(stack: embassy_net::Stack<'_>, mut scratch: Scratch<'_>, sd: &SdStorage) -> ! {
    loop {
        match serve_one_request(stack, &mut scratch, sd).await {
            ServerEvent::Uploaded { name, name_len } => {
                let fname = core::str::from_utf8(&name[..name_len as usize]).unwrap_or("???");
                info!("upload: file saved as '{}'", fname);
            }
            ServerEvent::UploadFailed => {
                info!("upload: file upload failed");
            }
            ServerEvent::Deleted { name, name_len } => {
                let fname = core::str::from_utf8(&name[..name_len as usize]).unwrap_or("???");
                info!("upload: deleted '{}'", fname);
            }
            ServerEvent::DeleteFailed => {
                info!("upload: file delete failed");
            }
            ServerEvent::Nothing => {}
        }
    }
}

async fn serve_one_request(
    stack: embassy_net::Stack<'_>,
    scratch: &mut Scratch<'_>,
    sd: &SdStorage,
) -> ServerEvent {
    let Scratch {
        tcp_rx,
        tcp_tx,
        work,
        request,
    } = scratch;
    let mut socket = TcpSocket::new(stack, tcp_rx, tcp_tx);
    socket.set_timeout(Some(Duration::from_secs(HTTP_TIMEOUT_SECS)));

    if socket
        .accept(IpListenEndpoint {
            addr: None,
            port: 80,
        })
        .await
        .is_err()
    {
        Timer::after(Duration::from_millis(ACCEPT_RETRY_MS)).await;
        return ServerEvent::Nothing;
    }

    let event = http::serve_request(&mut socket, sd, request, work).await;
    close_socket(&mut socket).await;
    event
}

async fn close_socket(socket: &mut TcpSocket<'_>) {
    Timer::after(Duration::from_millis(SOCKET_CLOSE_DELAY_MS)).await;
    socket.close();
    Timer::after(Duration::from_millis(SOCKET_CLOSE_DELAY_MS)).await;
    socket.abort();
}

struct MdnsSocket<'a>(UdpSocket<'a>);

impl mdns::Datagrams for MdnsSocket<'_> {
    type Error = MdnsError;

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, MdnsError> {
        let (n, _remote) = self.0.recv_from(buf).await.map_err(MdnsError::Recv)?;
        Ok(n)
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), MdnsError> {
        let [a, b, c, d] = mdns::GROUP;
        let dest = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(a, b, c, d)), mdns::PORT);
        self.0.send_to(data, dest).await.map_err(|e| {
            info!("upload: mDNS send failed: {:?}", e);
            MdnsError::Send
        })?;
        info!("upload: mDNS answered pulp.local");
        Ok(())
    }
}

#[derive(Debug)]
enum MdnsError {
    Recv(RecvError),
    Send,
}

// One socket for the whole session: bound to the mDNS port and joined to the
// group, so it receives the multicast queries and sends the answers. If it
// cannot be set up or receiving fails, mDNS stays off and HTTP keeps serving.
async fn serve_mdns(stack: embassy_net::Stack<'_>, ip_octets: [u8; 4]) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; mdns::RECV_BUF_LEN];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_buf = [0u8; mdns::RESPONSE_LEN];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    let [a, b, c, d] = mdns::GROUP;
    if let Err(e) = socket.bind(mdns::PORT) {
        info!("upload: mDNS bind failed: {:?}", e);
    } else if let Err(e) = stack.join_multicast_group(Ipv4Address::new(a, b, c, d)) {
        info!("upload: mDNS group join failed: {:?}", e);
    } else {
        let Err(e) = mdns::serve(&mut MdnsSocket(socket), ip_octets).await;
        info!("upload: mDNS stopped: {:?}", e);
    }
    core::future::pending().await
}

// `session::run` has already checked the lengths; this converts to the radio
// crate's bounded types.
fn station_config(ssid: &str, password: &str) -> Result<StationConfig, ConnectError> {
    match (Ssid::try_from(ssid), Password::try_from(password)) {
        (Ok(s), Ok(p)) => Ok(StationConfig::default()
            .with_ssid(s)
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(p))),
        _ => Err(ConnectError::InvalidCredentials),
    }
}

async fn show_error(
    epd: &mut Epd,
    strip: &mut StripBuffer,
    delay: &mut Delay,
    heading: &'static BitmapFont,
    body: &'static BitmapFont,
    error: ConnectError,
    bumps: &ButtonFeedback,
) {
    render_screen(
        epd,
        strip,
        delay,
        heading,
        body,
        error.lines(),
        Some("Press BACK to exit"),
        None,
        bumps,
        false,
    )
    .await;
    drain_until_back().await;
}

async fn drain_until_back() {
    let mapper = ButtonMapper::new();
    loop {
        let hw = tasks::INPUT_EVENTS.receive().await;
        let ev = mapper.map_event(hw);
        if matches!(
            ev,
            ActionEvent::Press(Action::Back) | ActionEvent::LongPress(Action::Back)
        ) {
            return;
        }
    }
}

async fn render_screen(
    epd: &mut Epd,
    strip: &mut StripBuffer,
    delay: &mut Delay,
    heading: &'static BitmapFont,
    body: &'static BitmapFont,
    lines: &[&str],
    footer: Option<&str>,
    qr: Option<&QrSymbol>,
    bumps: &ButtonFeedback,
    full_refresh: bool,
) {
    let heading_h = heading.line_height;
    let body_h = body.line_height;
    let body_stride = body_h + BODY_LINE_GAP;

    let heading_region = Region::new(HEADING_X, CONTENT_TOP + 12, HEADING_W, heading_h);

    let body_area_top = CONTENT_TOP + 12 + heading_h + 40;
    let body_area_bottom = FOOTER_Y.saturating_sub(20);
    let body_area_h = body_area_bottom.saturating_sub(body_area_top);
    let total_body_h = if lines.is_empty() {
        0
    } else {
        (lines.len() as u16 - 1) * body_stride + body_h
    };
    // with a QR code the lines move to the top of the area and the code
    // takes the space below them
    let body_start_y = if qr.is_some() {
        body_area_top
    } else {
        body_area_top + body_area_h.saturating_sub(total_body_h) / 2
    };
    let qr_top = body_start_y + total_body_h + QR_GAP;
    // `draw` centres the code in the region
    let qr_region = Region::new(
        BODY_X,
        qr_top,
        BODY_W,
        body_area_bottom.saturating_sub(qr_top).min(QR_MAX_SIDE),
    );

    let footer_region = Region::new(BODY_X, FOOTER_Y, BODY_W, body_h);

    let draw = |s: &mut StripBuffer| {
        BitmapLabel::new(heading_region, "Upload", heading)
            .alignment(Alignment::CenterLeft)
            .draw(s)
            .unwrap();

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let y = body_start_y + (i as u16) * body_stride;
            let region = Region::new(BODY_X, y, BODY_W, body_h);
            BitmapLabel::new(region, line, body)
                .alignment(Alignment::Center)
                .draw(s)
                .unwrap();
        }

        if let Some(qr) = qr {
            qr.draw(s, qr_region);
        }

        if let Some(text) = footer {
            BitmapLabel::new(footer_region, text, body)
                .alignment(Alignment::Center)
                .draw(s)
                .unwrap();
        }

        bumps.draw(s);
    };

    if full_refresh {
        board::full_refresh_screen(epd, strip, delay, &draw).await;
    } else {
        board::partial_refresh_screen(epd, strip, delay, &draw).await;
    }
}
