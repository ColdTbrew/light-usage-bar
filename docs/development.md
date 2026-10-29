# 개발과 검증

이 저장소에는 공통 모델, 제공자 어댑터, 갱신 서비스, AppKit 호스트와 GPUI 패널이 있다. macOS 전용 코드가 제외되는 Linux 테스트로 앱 실행을 검증할 수 없다.

## 도구와 작업 규칙

Rust `1.99.1`은 `rust-toolchain.toml`에 고정했다. GPUI는 `0.2.2`다. `Cargo.lock`을 사용한다. `cargo`를 찾지 못하면 셸에서 `export PATH="$HOME/.cargo/bin:$PATH"`를 실행한다.

저장소의 `AGENTS.md`에 따라 [pstack-codex](https://github.com/ColdTbrew/pstack-codex)를 조사, 구현, 검토와 배포에 적용한다. Codex 작업은 `$pstack-codex:poteto-mode`로 시작한다. Python 작업에는 `uv`를 사용한다.

DGX Spark GB10, Ubuntu ARM64는 공통 로직 개발과 테스트 환경이다. macOS 빌드와 UI 검증은 Apple Silicon Mac에서 수행한다. 전체 Xcode와 Metal toolchain 준비는 [설치 문서](install.md)에 있다.

## 정적 검사와 합성 테스트

저장소 루트에서 실행한다.

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run --locked -p usage-core --example cache_lifecycle
```

합성 테스트는 계정 전환, 마지막 성공 값 보존, 오래된 응답 거부, 파싱, IPC와 갱신 제한을 검사한다. Antigravity는 source 변경, status 계정 확인, 누락 identity 거절과 일시적인 관찰 실패의 stale 값 보존도 검사한다. 실제 서비스 수락, Keychain 접근 허용, GPUI 화면 표시와 메모리 성능은 이 검사로 확인할 수 없다. Linux에서는 AppKit, GPUI와 Antigravity native 탐색의 macOS 구현이 컴파일 대상에서 제외된다.

macOS에서는 실행 파일을 별도로 빌드한다.

```sh
cargo build --locked --workspace --bins
```

2026-10-30 기준 M3 Pro MacBook Pro, RAM 18 GiB, macOS 26.6.2, Xcode 26.5에서 전체 실행 파일 빌드와 Antigravity 변경을 포함한 워크스페이스 테스트 75개를 통과했다. macOS 전용 테스트 초기화 수정 뒤 제공자 테스트와 전체 대상의 엄격한 Clippy를 다시 통과했다. fmt, release 패키징, 최종 번들의 deep strict 서명 검사와 ZIP checksum 검사도 통과했다. `DEVELOPER_DIR`로 전체 Xcode를 선택하고 Metal toolchain을 설치했다.

이전 head `801a1048`의 macOS CI와 다운로드 artifact 검증은 통과했다. 이후 Antigravity 변경의 원격 CI는 아직 실행하지 않았다. CI 통과 여부는 해당 커밋의 실제 check 결과로 판단한다. 로컬 검사 기록을 원격 CI 통과로 표현하지 않는다.

## 인증 없는 UI 확인

macOS에서 실행 파일 둘을 같은 debug 또는 release 디렉터리에 빌드한 뒤 실행한다.

```sh
cargo build --locked -p usage-app -p usage-panel
./target/debug/light-usage-bar --demo
./target/debug/light-usage-bar --smoke-test
```

`--demo`는 합성 스냅샷을 사용하고 실제 제공자에 접속하지 않는다. `--smoke-test`도 합성 데이터를 사용하며 패널 시작, 준비 응답, 종료와 회수 경로를 확인하려는 명령이다. 명령의 구현과 통과 결과를 구분한다. 현재 잠긴 Mac 화면에서의 실행은 준비 응답을 확인하지 못해 판정 보류다. 잠금 해제 후 재실행이 필요하다. 상세 실행 결과는 [배포 상태](release.md)에 있다. 이 검사는 사람이 메뉴와 패널을 클릭하는 검증을 대체하지 않는다.

앱 번들의 호스트 실행 파일에도 같은 옵션을 전달할 수 있다. 패널 실행 파일을 직접 실행하면 호스트의 초기 메시지가 없으므로 정상 앱 사용과 다르다. 내부 `--credential-helper`는 인증 정보를 출력하는 전용 프로토콜이므로 진단 명령으로 실행하지 않는다.

## 실계정 조회

기존 공식 도구의 로그인 상태에서 [읽기 전용 진단](providers.md#읽기-전용-진단)을 사용한다. Antigravity는 지원 경로의 앱을 실행한 상태에서 확인한다. 기본 출력을 유지하고 인증 파일과 응답 원문을 수집하지 않는다. Claude 접근 거부나 Antigravity identity 확인 실패를 조회 성공으로 기록하지 않는다.

설치된 Antigravity 2.24.0은 status에 email이 없어 `NeedsVerification`이다. 앱 종료 시에는 `TemporarilyUnavailable`을 확인했다. 다른 저장소에서 계정을 추정하거나 합성 identity를 넣어 실제 quota 검증으로 기록하지 않는다. 실제 quota 조회 성공과 공식 화면 대조는 남아 있다.

## 결과 기록

[배포 상태](release.md)에 실행 환경, 빌드와 UI 확인, 제공자 확인, 패키지 검증을 나누어 기록한다. [벤치마크 기록](benchmarks.md)에는 실제 측정값만 넣는다. 공개 기록에서 계정, 토큰과 개인 사용량을 제거한다. 라이선스는 소유자의 결정 전까지 미정으로 유지한다.
