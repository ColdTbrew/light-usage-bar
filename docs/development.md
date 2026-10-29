# 개발 및 검증

현재 구현 범위는 `crates/usage-core`의 공통 사용량 모델과 메모리 캐시다. 실제 제공자에 접속하거나 macOS 메뉴 막대를 실행하는 앱은 아직 없다.

## 개발 도구 준비

Rust 버전은 `rust-toolchain.toml`의 `1.99.1`로 고정한다. Rust 설치는 [공식 rustup 안내](https://rust-lang.org/tools/install/)를 따른다. 이 워크스페이스는 외부 Rust crate에 의존하지 않는다.

이번 작업에서 DGX와 Mac의 셸 설정은 수정하지 않았다. 해당 터미널에서 `cargo`를 찾지 못하면 `export PATH="$HOME/.cargo/bin:$PATH"`를 실행한다.

저장소 작업에는 [pstack-codex](https://github.com/ColdTbrew/pstack-codex)를 적용한다. 설치 명령은 다음과 같다.

```sh
codex plugin marketplace add ColdTbrew/pstack-codex --ref main
codex plugin add pstack-codex@pstack-codex
git clone https://github.com/ColdTbrew/pstack-codex.git
bash pstack-codex/scripts/install-agents.sh
```

설치 후 새 Codex 작업에서 `$pstack-codex:poteto-mode`를 사용한다. `AGENTS.md`는 조사, 구현, 문서, 검토와 배포 작업에도 이 규칙을 적용한다. Python 작업이 필요하면 `uv`를 사용한다.

## 공통 라이브러리 확인

저장소 루트에서 실행한다.

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run --locked -p usage-core --example cache_lifecycle
```

예제는 합성 사용량으로 성공, 조회 실패, 계정 전환을 실행한다. 실패 뒤에는 마지막 사용량을 유지하고, 계정이 바뀌면 이전 계정 값을 제거한다. 인증 정보와 네트워크는 사용하지 않는다.

CI는 `macos-15`에서 같은 명령을 실행한다. GitHub가 문서화한 이 runner의 CPU는 ARM64다. [Runner 명세](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)를 참고한다. 공통 라이브러리 검증이며, AppKit·GPUI 앱 실행과 RAM 비교를 대신하지 않는다.

## 첫 구현의 설계 선택

사용량 snapshot은 성공한 조회 결과를 보존한다. 캐시는 진행 중인 요청과 실패 상태를 별도로 관리하고, 조회 시각과 만료 시각으로 최신성을 판단한다.

| 후보 | 판단 |
| --- | --- |
| 사용량과 조회 상태를 한 레코드에서 계속 덮어쓰기 | 실패 처리 중 마지막 정상 값과 측정 시각을 잃기 쉬워 채택하지 않는다. |
| 성공 snapshot과 조회 상태 분리 | 마지막 정상 값, 새 오류, 오래됨을 함께 표현할 수 있어 채택한다. |

`Model the Domain` 원칙에 따라 알 수 없는 값, 사용·잔여 방향, 초기화·체험 만료, 조회 상태를 타입으로 구분한다. `Test Behavior, Not Implementation` 원칙에 따라 반환된 값과 계정 전환 결과를 검증한다.

독립적인 두 구현안을 비교해 작은 직접 호출형 캐시를 선택했다. 다른 후보에서 조회 상태를 하나의 내부 enum으로 표현하는 방식을 채택했다. 제공자 window 식별자·시작 시각과 계산값의 근거 설명은 검토에서 발견한 누락을 보완한 것이다. 요청별 `Arc` 식별자로 이전 응답과 다른 캐시의 응답을 차단한다. 물리적인 요청 취소를 기다리는 큐, 전역 요청 번호와 IPC용 표시 타입은 이번 범위에 넣지 않았다.

Snapshot의 출처와 관측 시각은 모든 window에 공통으로 적용된다. 서로 다른 출처나 관측 시각의 값을 합칠 때는 adapter에서 이 제약을 고려해야 한다. 계산값의 설명에는 입력값과 가정을 기록하며, 비밀값과 원문 계정 정보는 넣지 않는다. 라이브러리는 설명이 비어 있는지만 검사하며 계산의 사실 여부를 검증하지 않는다.

## 검증 범위

2026-10-29의 로컬 검증 장비는 DGX Spark GB10, Ubuntu ARM64다. 이 환경의 Rust 테스트 결과는 공통 로직에만 적용된다. Linux 앱 지원을 추가한 것은 아니다.

macOS 빌드와 앱 실행에는 tailnet의 MacBook Pro를 사용한다. SSH 접속 계정은 `coldbrew@macbookpro`다. 작업 파일은 기존 체크아웃과 분리한 디렉터리에 복사한다. 현재 확인한 장비는 `Mac15,6`, RAM 18 GiB, macOS `26.6.2`와 빌드 `25G83`, ARM64다. Command Line Tools 경로는 `/Library/Developer/CommandLineTools`다.

현재 DGX에는 전용 SSH 별칭 `light-usage-bar-mac`을 설정했다. `ssh light-usage-bar-mac`으로 접속하며, 다른 개발 장비에서는 자신의 SSH 설정을 사용한다. 개인키는 저장소에 포함하지 않는다.

2026-10-29에 두 장비에서 Rust `1.99.1`로 다음 확인을 완료했다. Mac에 복사한 라이브러리, 테스트와 예제 파일의 SHA-256이 DGX 원본과 일치했다.

| 확인 항목 | DGX Ubuntu ARM64 | MacBook Pro ARM64 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 통과 | 통과 |
| `cargo test --workspace --all-targets --locked` | 동작 테스트 16개 통과 | 동작 테스트 16개 통과 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 통과 | 통과 |
| `cargo run --locked -p usage-core --example cache_lifecycle` | 42% 보존 후 계정 전환 시 값 없음 | 42% 보존 후 계정 전환 시 값 없음 |

이 기록은 네이티브 Mac에서 공통 라이브러리와 콘솔 예제를 실행한 결과다. GitHub CI 상태는 각 PR의 check 결과에서 별도로 확인한다.

실제 계정 인증, 제공자 응답 의미, macOS 메뉴 막대, GPUI 수명, 앱 번들, 최소 macOS 버전과 메모리 사용은 검증하지 않았다. 이 항목의 완료 조건은 [로드맵](roadmap.md)에 유지한다. 라이선스도 아직 선택하지 않았다.
