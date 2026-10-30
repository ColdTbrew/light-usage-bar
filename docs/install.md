# 빌드와 설치

현재는 개발자용 미리보기 배포 후보다. 공개된 정식 설치 파일은 아직 없다. 아래 절차는 저장소에서 직접 패키지를 만드는 방법이다. 기능별 확인 결과와 남은 항목은 [배포 상태](release.md)에 있다.

## 빌드 환경 준비

Apple Silicon Mac과 macOS 26 이상을 사용한다. 검증한 장비는 M3 Pro MacBook Pro, RAM 18 GiB, macOS 26.6.2다. Intel, macOS 25 이하, Windows와 Linux는 앱 지원 대상이 아니다.

Rust 버전은 `rust-toolchain.toml`의 `1.99.1`이다. 전체 Xcode가 필요하다. 이번 컴파일 확인에는 Xcode 26.5를 사용했다. Command Line Tools만 선택한 환경은 GPUI Metal 빌드에 충분하지 않았다.

저장소 루트에서 실행한다. Xcode 설치 경로가 다르면 첫 줄을 바꾼다.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcodebuild -downloadComponent MetalToolchain
rustup show active-toolchain
```

첫 빌드에는 Rust 의존성과 Metal toolchain 다운로드가 필요하다.

## 앱 번들 만들기

```sh
bash scripts/package-macos.sh
```

스크립트는 `MACOSX_DEPLOYMENT_TARGET=26.0`을 설정한다. `cargo build --locked --release -p usage-app -p usage-panel`로 실행 파일을 만들고 다음 경로에 산출물을 쓴다.

```text
dist/Light Usage Bar.app
dist/Light-Usage-Bar-0.1.0-macOS-arm64.zip
dist/Light-Usage-Bar-0.1.0-macOS-arm64.zip.sha256
```

파일 이름의 버전은 `crates/usage-app/Cargo.toml`에서 읽는다. 패널은 앱의 `Contents/Helpers/light-usage-panel`에 포함된다. 실행 파일 하나만 복사하면 상세 패널을 열 수 없다.

## 서명과 확인

`SIGNING_IDENTITY`를 지정하지 않으면 스크립트는 ad hoc 서명한다. 이 서명은 Developer ID 서명이나 Apple 공증이 아니다. 현재 관찰한 인증서는 Apple Development였으며 Developer ID 배포 인증서는 확인하지 못했다.

사용할 배포 인증서가 있다면 다음과 같이 명시한다.

```sh
SIGNING_IDENTITY='Developer ID Application: YOUR NAME (TEAMID)' bash scripts/package-macos.sh
```

스크립트는 서명을 검증하지만 공증 제출과 stapling은 수행하지 않는다. 배포자는 별도로 공증 절차와 Gatekeeper 검증을 완료해야 한다. ZIP의 체크섬을 다음과 같이 확인한다.

```sh
(cd dist && shasum -a 256 -c Light-Usage-Bar-0.1.0-macOS-arm64.zip.sha256)
codesign --verify --deep --strict --verbose=2 'dist/Light Usage Bar.app'
```

## 로컬 설치와 실행

1. 생성한 `Light Usage Bar.app` 전체를 Applications 폴더에 복사한다.
2. 앱을 실행한다. Dock 창 대신 메뉴 막대의 `Usage` 항목을 확인한다.
3. 메뉴에서 `Open usage panel`을 선택한다.
4. 제공자별 조회 상태를 확인한다. 인증이 필요하면 공식 도구에서 로그인한다.

공증하지 않은 앱은 Gatekeeper가 차단할 수 있다. 전역 보안 설정을 끄거나 quarantine을 일괄 제거하지 않는다. 현재 배포 후보를 일반 사용자에게 전달하기 전 공증과 실제 설치 실행을 확인해야 한다.

사용자 지정 인증 디렉터리가 필요하면 [제공자 선택 규칙](providers.md#인증-정보-선택)을 읽는다. Finder 실행은 터미널 환경 변수를 그대로 상속하지 않는다. 로컬 실행 파일에 필요한 환경 변수를 지정해 실행할 수 있다.

```sh
CODEX_HOME=/path/to/codex '/Applications/Light Usage Bar.app/Contents/MacOS/light-usage-bar'
```

## 종료와 삭제

메뉴의 `Quit Light Usage Bar`로 종료한다. 패널에는 `Quit` 동작도 구현했다. 실제 조작 검증 여부는 [배포 상태](release.md)에 기록한다. 앱을 종료한 뒤 Applications의 앱 번들을 삭제한다. 선택적으로 [앱 설정 파일](privacy.md#로컬-저장과-프로세스)을 삭제할 수 있다.
