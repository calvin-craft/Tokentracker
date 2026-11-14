# TokenTracker

AI 구독 서비스의 사용량 한도를 바탕화면에 띄워 두는 데스크톱 위젯입니다.
5시간·주간 한도를 얼마나 썼는지, 언제 리셋되는지를 한눈에 볼 수 있습니다.
Windows, macOS, Linux에서 동작합니다.

[English](#english) · [다운로드](https://github.com/calvin-craft/Tokentracker/releases/latest)

> [!WARNING]
> **비공식 도구이며, 본인 책임으로 사용해 주세요.**
> TokenTracker는 OpenAI, Anthropic, Google, xAI 등 어떤 회사와도 관계없는 개인 프로젝트입니다.
> 사용량은 공식 API가 아니라 각 서비스 웹사이트가 내부적으로 쓰는 주소를 로그인된 세션으로 읽어 옵니다. 이는 각 서비스의 이용약관에 어긋날 수 있습니다.
> 이 앱을 사용하면서 생기는 계정 제한·정지나 그 밖의 어떤 문제에 대해서도 개발자는 책임지지 않습니다.

## 지원 서비스

| 서비스 | 보여 주는 한도 | 읽는 방법 |
|---|---|---|
| **Codex (ChatGPT)** | 5시간, 주간 | 이 컴퓨터의 Codex 앱 로그인을 그대로 사용합니다. 계정을 더 추가하면 chatgpt.com에 따로 로그인합니다. |
| **Claude** | 5시간, 주간 (모델별 주간 포함) | 위젯 안에서 claude.ai에 한 번 로그인합니다. |
| **Gemini** | 5시간, 주간 | 위젯 안에서 Google 계정으로 한 번 로그인합니다. |
| **Antigravity** | Gemini 모델 / 그 밖의 모델(Others) | 이 컴퓨터의 Antigravity 앱 로그인을 그대로 사용합니다. Antigravity가 꺼져 있으면 마지막으로 확인한 값을 보여 줍니다. |
| **Grok** | 주간 사용량 | 위젯 안에서 grok.com에 로그인합니다. 아직 실제 계정으로 확인하지 못한 기능입니다. |

한 서비스에 계정을 여러 개 추가해 함께 볼 수도 있습니다(Antigravity 제외).

## 기능

- 한도별 사용률 막대와 리셋까지 남은 시간
- **다음 결제일**: 설정에서 계정마다 매월 결제일을 정하면 날짜와 남은 일수(D-day)를 보여 줍니다. 결제일은 직접 입력하며, 서비스에서 따로 읽어 오지 않습니다.
- **간략 보기**: 계정마다 한 줄로 표시
- **위젯 위치**: 바탕화면에 고정 / 항상 위 / 배경화면 모드(아이콘 뒤, Windows 11 24H2 이상 전용)
- 사용률 80%, 95%에 도달하면 알림
- 갱신 주기(3–30분, 기본 5분), 배경 불투명도, 로그인 시 자동 실행
- **요청 최소화**: 각 서비스에 보내는 요청을 줄였습니다. Grok은 따로 요청하지 않고, Antigravity가 꺼져 있으면 마지막 값을 보여 줍니다. 꺼져 있을 때도 새로 확인하려면 설정에서 "Antigravity가 꺼져 있을 때도 확인"(실험적)을 켭니다.
- 한국어 / English (처음에는 시스템 언어를 따름)

## 설치

[Releases](https://github.com/calvin-craft/Tokentracker/releases/latest)에서 OS에 맞는 파일을 받습니다.

| OS | 설치 파일 |
|---|---|
| Windows 10/11 (64비트) | `TokenTracker_x.y.z_x64-setup.exe` |
| macOS 11 이상 (Intel·Apple Silicon) | `TokenTracker_x.y.z_universal.dmg` |
| Linux (Debian/Ubuntu) | `TokenTracker_x.y.z_amd64.deb` |
| Linux (Fedora 등) | `TokenTracker-x.y.z-1.x86_64.rpm` |
| Linux (그 밖) | `TokenTracker_x.y.z_amd64.AppImage` |

> macOS와 Linux 설치 파일은 GitHub Actions에서 빌드와 테스트를 통과했지만, 아직 실제 기기에서 실행해 보지 않았습니다. 문제가 있으면 Issues에 알려 주세요.

### Windows

1. `TokenTracker_x.y.z_x64-setup.exe`를 받아 실행합니다.
2. "Windows의 PC 보호" 창이 뜨면 **추가 정보 → 실행**을 누릅니다. 코드 서명을 하지 않은 앱이라 나오는 경고입니다.
3. 위젯의 톱니바퀴(설정)에서 쓰는 서비스에 로그인합니다.

위젯이 다른 창 뒤에 가려져 있을 때는 작업 표시줄 트레이 아이콘을 누르면 앞으로 나옵니다.

### macOS

1. `.dmg`를 열어 TokenTracker를 응용 프로그램 폴더로 옮깁니다.
2. 서명하지 않은 앱이라 처음에는 열리지 않습니다. 앱을 **Control-클릭 → 열기**를 누르거나, 시스템 설정 → 개인정보 보호 및 보안에서 **그래도 열기**를 누릅니다.
3. Dock 아이콘 없이 **메뉴 막대**에 아이콘으로 실행됩니다. 위젯과 설정은 메뉴 막대 아이콘에서 엽니다.

같은 서비스의 계정을 여러 개 따로 로그인하려면 macOS 14 이상이 필요합니다. 그보다 낮은 버전에서는 추가한 계정이 기본 계정과 로그인을 함께 씁니다.

### Linux

- Debian/Ubuntu는 `.deb`, Fedora 등은 `.rpm`을 설치하거나, `.AppImage`에 실행 권한을 주고 실행합니다.
- 트레이 아이콘에 AppIndicator가 필요합니다(`libayatana-appindicator3-1`). GNOME에서는 AppIndicator 확장이 있어야 아이콘이 보입니다.
- 일부 NVIDIA·Wayland 환경에서 창이 비어 보이는 문제를 피하려고 WebKitGTK의 DMA-BUF 렌더러를 기본으로 끕니다. 직접 `WEBKIT_DISABLE_DMABUF_RENDERER`를 지정하면 그 값을 따릅니다.

## 개인정보

- 로그인은 각 서비스의 공식 로그인 화면에서 직접 합니다. TokenTracker는 비밀번호를 받거나 저장하지 않습니다.
- 사용량은 각 서비스에 로그인된 세션으로 그 서비스에만 요청합니다. 다른 서버로 보내는 데이터는 없습니다.
- 위젯이 가져오는 것은 사용률, 리셋 시각, 요금제 이름뿐입니다. 이름, 이메일 같은 계정 정보는 읽어 오지 않습니다.
- Codex와 Antigravity의 로그인 정보는 각 앱이 가진 것을 읽기만 하고, 복사하거나 따로 저장하지 않습니다. Antigravity가 꺼져 있을 때는 마지막으로 확인한 값을 보여 줍니다. 설정에서 "Antigravity가 꺼져 있을 때도 확인"을 켠 경우에만, Antigravity에 들어 있는 서버 프로그램을 몇 초간 실행해 한도를 물어본 뒤 종료합니다.
- Grok은 대시보드 페이지가 스스로 불러오는 사용량 데이터와 화면의 글자만 읽고, grok.com에 따로 요청을 보내지 않습니다.
- 앱 데이터는 모두 사용자 폴더에 저장되며, 설정에서 계정을 삭제하면 그 계정의 로그인 정보도 지워집니다.

| OS | 설정, 위젯 위치 | 로그인 세션(쿠키) |
|---|---|---|
| Windows | `%APPDATA%\com.tokentracker.desktop\` | `%LOCALAPPDATA%\com.tokentracker.desktop\` |
| macOS | `~/Library/Application Support/com.tokentracker.desktop/` | macOS의 WebKit 저장소 |
| Linux | `~/.config/com.tokentracker.desktop/` | `~/.local/share/com.tokentracker.desktop/` |

## 직접 빌드하기

필요한 것: [Node.js](https://nodejs.org/) 22 이상, [Rust](https://rustup.rs/), 그리고 OS별 [Tauri 준비 항목](https://tauri.app/start/prerequisites/)

- **Windows**: Microsoft C++ Build Tools, WebView2
- **macOS**: Xcode Command Line Tools
- **Linux (Debian/Ubuntu)**:
  ```bash
  sudo apt-get install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
    librsvg2-dev libdbus-1-dev libxdo-dev libssl-dev patchelf
  ```

```bash
npm install
npm run tauri dev     # 개발 모드로 실행
npm run tauri build   # 설치 파일 만들기 → src-tauri/target/release/bundle/
```

macOS에서 Intel·Apple Silicon 겸용으로 만들려면 `npm run tauri build -- --target universal-apple-darwin`을 씁니다.

**GitHub Actions**: `v`로 시작하는 태그(예: `v1.0.0`)를 푸시하거나 Actions 탭에서 `build`를 직접 실행하면 세 OS의 설치 파일을 빌드하고 테스트합니다. 결과물은 그 실행(run)의 Artifacts에서 받을 수 있습니다.

React + TypeScript(`src/`) 화면과 Rust(`src-tauri/src/`) 백엔드로 된 [Tauri 2](https://tauri.app/) 앱입니다. 서비스마다 파일이 하나씩 있습니다(`claude.rs`, `gemini.rs`, `codex.rs`, `chatgpt.rs`, `antigravity.rs`, `grok.rs`).

## 참고

- 각 서비스가 공개 API로 제공하는 정보가 아니라서, 서비스 쪽 화면이나 내부 주소가 바뀌면 일시적으로 값을 못 읽을 수 있습니다.
- 이 프로젝트는 OpenAI, Anthropic, Google, xAI와 관련이 없는 개인 프로젝트입니다.

---

## English

A desktop widget for Windows, macOS and Linux that shows how much of your AI subscription limits you have used: the 5-hour and weekly limits, and when each resets.

> [!WARNING]
> **Unofficial tool. Use at your own risk.**
> TokenTracker is a personal project and is not affiliated with OpenAI, Anthropic, Google, xAI or any other company.
> It reads usage through the internal web endpoints of each service with your signed-in session, not through an official API. This may go against the services' terms of use.
> The developer is not responsible for account limits, suspensions or any other problems that come from using this app.

### Services

| Service | Limits | How it is read |
|---|---|---|
| **Codex (ChatGPT)** | 5-hour, weekly | Uses the Codex app's login on this computer. Extra accounts sign in to chatgpt.com. |
| **Claude** | 5-hour, weekly (incl. per-model weekly) | Sign in to claude.ai once, inside the widget. |
| **Gemini** | 5-hour, weekly | Sign in with your Google account once, inside the widget. |
| **Antigravity** | Gemini models / Others | Uses the Antigravity app's login on this computer; while Antigravity is closed, the last values seen are shown. |
| **Grok** | Weekly usage | Sign in to grok.com inside the widget. Not yet tested with a real account. |

You can add several accounts of the same service (except Antigravity).

### Features

- Usage bars with time to reset; compact one-line-per-account view
- **Next billing date**: set each account's monthly billing day in Settings to see the date and days left. You enter it yourself; nothing is read from the services.
- Position: on the desktop / always on top / wallpaper mode (behind the icons; Windows 11 24H2+ only)
- Alerts at 80% and 95%; refresh interval (3–30 min, 5 by default), background opacity, start at login
- **Few requests**: Grok gets no requests of its own, and while Antigravity is closed its last values are shown. To check it anyway, turn on "Check Antigravity while it is closed" (experimental) in Settings.
- Korean / English (follows the system language at first)

### Install

Download the file for your OS from [Releases](https://github.com/calvin-craft/Tokentracker/releases/latest).

| OS | Package |
|---|---|
| Windows 10/11 (64-bit) | `TokenTracker_x.y.z_x64-setup.exe` |
| macOS 11+ (Intel and Apple Silicon) | `TokenTracker_x.y.z_universal.dmg` |
| Linux (Debian/Ubuntu) | `TokenTracker_x.y.z_amd64.deb` |
| Linux (Fedora and others) | `TokenTracker-x.y.z-1.x86_64.rpm` |
| Linux (any) | `TokenTracker_x.y.z_amd64.AppImage` |

> The macOS and Linux packages pass the GitHub Actions build and tests but have not been run on real machines yet. Please report problems in Issues.

**Windows**: run the installer. If SmartScreen appears, choose **More info → Run anyway** (the app is not code-signed). Then open Settings (gear icon) and sign in. Click the tray icon to bring the widget forward.

**macOS**: move TokenTracker to Applications. As the app is unsigned, the first time **Control-click → Open** it, or choose **Open Anyway** under System Settings → Privacy & Security. It runs from the **menu bar**, without a Dock icon. Separate logins for several accounts of one service need macOS 14 or later; on older versions added accounts share the built-in account's login.

**Linux**: install the `.deb` or `.rpm`, or make the `.AppImage` executable and run it. The tray icon needs AppIndicator (`libayatana-appindicator3-1`; on GNOME, the AppIndicator extension). WebKitGTK's DMA-BUF renderer is turned off by default to avoid blank windows on some NVIDIA and Wayland setups; set `WEBKIT_DISABLE_DMABUF_RENDERER` yourself to override.

### Privacy

- You sign in on each service's own sign-in page. TokenTracker never sees or stores passwords.
- Usage is requested only from each service itself, with your signed-in session. Nothing is sent anywhere else.
- Only percentages, reset times and the plan name are read — no names or e-mail addresses.
- The Codex and Antigravity logins are read from those apps, never copied. While Antigravity is closed, the last values seen are shown. Only if you turn on "Check Antigravity while it is closed" in the settings is its own language server started for a few seconds to ask for the quota, then stopped.
- For Grok, only the usage data the dashboard loads by itself and the text on the page are read; no requests of our own are sent to grok.com.
- App data lives in your user folder; removing an account deletes its session.

| OS | Settings, widget position | Sign-in sessions (cookies) |
|---|---|---|
| Windows | `%APPDATA%\com.tokentracker.desktop\` | `%LOCALAPPDATA%\com.tokentracker.desktop\` |
| macOS | `~/Library/Application Support/com.tokentracker.desktop/` | macOS's WebKit storage |
| Linux | `~/.config/com.tokentracker.desktop/` | `~/.local/share/com.tokentracker.desktop/` |

### Build from source

Requires Node.js 22+, Rust, and [Tauri's prerequisites](https://tauri.app/start/prerequisites/) for your OS (Windows: C++ Build Tools and WebView2; macOS: Xcode Command Line Tools; Linux: the packages listed above).

```bash
npm install
npm run tauri dev     # run in development
npm run tauri build   # packages in src-tauri/target/release/bundle/
```

For a universal macOS build: `npm run tauri build -- --target universal-apple-darwin`.

**GitHub Actions**: pushing a tag that starts with `v` (e.g. `v1.0.0`), or running `build` by hand from the Actions tab, builds and tests the packages for all three systems and attaches them to the run as artifacts.

A [Tauri 2](https://tauri.app/) app: React + TypeScript UI in `src/`, Rust backend in `src-tauri/src/` with one file per service.

### Notes

- These limits are not offered through public APIs, so a change on a service's side can break reading them until the app is updated.
- An independent personal project, not affiliated with OpenAI, Anthropic, Google or xAI.
