# 연동 조사 출처

초기 조사일은 2026-10-25다. 아래 목록은 초기 조사와 이후 CLI 전환 때 공개 소스와 문서를 읽은 기록이다. 현재 구현과 2026-10-29의 실계정 조회 결과는 [제공자 참조](providers.md)와 [배포 상태](release.md)에 있다. RAM과 CPU 비교는 아직 수행하지 않았다.

서비스 공식 문서는 제품·플랜 의미의 근거로, 공개 프로그램의 코드는 그 프로그램이 사용하는 경로의 근거로 구분한다. GitHub issue의 제안이나 관리자 설명은 완료된 구현 또는 실제 계정 성공 증거가 아니다. `main` URL과 open 상태는 확인일 이후 바뀔 수 있다. 구현 시 실제로 참고한 commit과 버전을 별도로 기록한다.

## 구현 시 고정한 소스

2026-10-29 구현 조사와 2026-10-30 Claude CLI 전환 조사에서는 아래 revision을 사용했다. 이후 `main`의 변경을 현재 구현 근거로 간주하지 않는다.

- [OpenAI Codex b75d1e8](https://github.com/openai/codex/tree/b75d1e89665d256bf2c674b9590776f440c9bae7)의 passive WHAM 조회와 인증 저장 형식.
- [xAI grok-build 2bdd1d6](https://github.com/xai-org/grok-build/tree/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8)의 OIDC, `bot_client` handshake와 `bot.usage`.
- [CodexBar 5f93dc3](https://github.com/steipete/CodexBar/tree/5f93dc3068b3bee3f8753f3772614cc2949c335d)의 Claude CLI source planner, 상태 probe, CLI 세션과 Antigravity summary 파서.

## Claude

| ID | 1차 자료 | 확인한 내용과 한계 |
| --- | --- | --- |
| C1 | [CodexBar `ClaudeSourcePlanner.swift`](https://github.com/steipete/CodexBar/blob/5f93dc3068b3bee3f8753f3772614cc2949c335d/Sources/CodexBarCore/Providers/Claude/ClaudeSourcePlanner.swift) | CLI와 다른 source 선택을 조사한 고정 revision. Light Usage Bar는 Claude CLI만 사용하며 OAuth fallback을 두지 않는다. |
| C2 | [CodexBar `ClaudeStatusProbe.swift`](https://github.com/steipete/CodexBar/blob/5f93dc3068b3bee3f8753f3772614cc2949c335d/Sources/CodexBarCore/Providers/Claude/ClaudeStatusProbe.swift), [`ClaudeCLISession.swift`](https://github.com/steipete/CodexBar/blob/5f93dc3068b3bee3f8753f3772614cc2949c335d/Sources/CodexBarCore/Providers/Claude/ClaudeCLISession.swift) | `/usage` 화면 해석과 PTY 세션 수명을 조사한 구현 자료. 공식 API 계약이나 Light Usage Bar의 실제 수집 성공 증거는 아니다. |
| C3 | [claudebar](https://github.com/gautham-v/claudebar) | Rust + GPUI로 만든 Claude 중심 macOS 메뉴 막대 앱의 선행 사례. 기능·스택의 근거이며 Light Usage Bar와 비교한 RAM 측정값은 확보하지 않았다. |

## Codex

| ID | 1차 자료 | 확인한 내용과 한계 |
| --- | --- | --- |
| O1 | [CodexBar: `docs/codex.md`](https://github.com/steipete/CodexBar/blob/main/docs/codex.md) | `wham/usage`, `auth.json`, account/workspace 경계, OAuth와 CLI RPC, 로컬 비용 기록의 구분. native credential 쓰기·갱신 소유권에 관한 현행 설명 포함. |
| O2 | [Aura: `crates/aura-core/src/quota/codex.rs`](https://github.com/Rfluid/aura/blob/main/crates/aura-core/src/quota/codex.rs) | 직접 usage API와 로컬 `token_count.rate_limits` 대체 경로의 Rust 구현. 로컬 기록은 과거 관측값일 수 있다. |
| O3 | [CodexBar: `CodexStatusProbe.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Codex/CodexStatusProbe.swift) | CLI 상태 조회 구현의 참고 위치. CLI 프로세스를 사용하지 않는 직접 HTTP 경로와 구분한다. |

## Grok Bot

### 구독과 제품의 의미

| ID | 1차 자료 | 확인한 내용과 한계 |
| --- | --- | --- |
| G1 | [xAI: X Subscription의 Grok Bot](https://docs.x.ai/x-subscription/grok-bot) | 새 공유 주간 pool과 기존 linked 구독의 차이, 확인할 dashboard의 맥락. 각 계정의 migration 상태는 따로 확인해야 한다. |
| G2 | [xAI: Grok Bot FAQ](https://docs.x.ai/grok-bot/faq) | Grok Bot 제품·사용량 의미를 확인하는 공식 출발점. 내부 RPC의 외부 앱 사용 안정성을 보장하는 문서는 아니다. |
| G3 | [Cursor: Grok Bot plans](https://cursor.com/help/grok-bot/plans) | 기존 linked 플랜과 새 통합 구독의 quota 해석 차이를 교차 확인한다. Cursor는 Light Usage Bar의 지원 대상 provider가 아니다. |

### xAI 공식 Rust 프로토콜과 인증

| ID | 1차 자료 | 확인한 내용과 한계 |
| --- | --- | --- |
| G4 | [grok-build: `xai-grok-shell-base/src/env.rs`](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-shell-base/src/env.rs) | 프로덕션 Computer Hub WebSocket 주소 `wss://computer-hub.grok.com/v1/tools`. |
| G5 | [grok-build: `xai-tool-protocol/src/handshake.rs`](https://github.com/xai-org/grok-build/blob/main/crates/common/xai-tool-protocol/src/handshake.rs), [`connection.rs`](https://github.com/xai-org/grok-build/blob/main/crates/common/xai-tool-protocol/src/connection.rs) | protocol handshake, `bot_client`, 서버의 지원 기능 응답 구조. |
| G6 | [grok-build: `xai-tool-protocol/src/methods.rs`](https://github.com/xai-org/grok-build/blob/main/crates/common/xai-tool-protocol/src/methods.rs), [`bot_relay.rs`](https://github.com/xai-org/grok-build/blob/main/crates/common/xai-tool-protocol/src/bot_relay.rs) | `bot.usage`, 호출자별 주간 사용량, cloud machine을 깨우지 않는 조회, optional·over-100 비율 및 trial/reset 필드. |
| G7 | [grok-build: `xai-computer-hub-sdk/src/auth.rs`](https://github.com/xai-org/grok-build/blob/main/crates/common/xai-computer-hub-sdk/src/auth.rs) | WebSocket upgrade에 Bearer 또는 준비된 header를 붙이는 SDK 기능. 특정 계정 token의 production 수락 증거는 아니다. |
| G8 | [grok-build: `xai-grok-workspace/src/hub_auth/mod.rs`](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-workspace/src/hub_auth/mod.rs), [authentication guide](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md) | 공식 클라이언트의 `~/.grok/auth.json` OIDC 읽기·갱신·저장 소유권. 이를 읽는 별도 앱의 `bot_client` 호환성은 미검증이다. |
| G9 | [CodexBar issue #4633](https://github.com/steipete/CodexBar/issues/4633), [관리자 댓글](https://github.com/steipete/CodexBar/issues/4633#issuecomment-6263895383) | first-party `bot.usage` 통합 요청. 확인일에 open이며 transport 및 실제 credential 검증이 남은 상태였다. 완료된 provider 구현으로 인용하지 않는다. |

### 서로 다른 사용량을 혼동하지 않기 위한 자료

| ID | 1차 자료 | 구분할 내용 |
| --- | --- | --- |
| G10 | [CodexBar: `docs/xai.md`](https://github.com/steipete/CodexBar/blob/main/docs/xai.md), [`docs/grok.md`](https://github.com/steipete/CodexBar/blob/main/docs/grok.md) | `xai`는 API prepaid balance·spend, `grok`는 Grok Build/SuperGrok credits 경로를 설명한다. 어느 쪽도 그 자체로 별도 Grok Bot meter를 증명하지 않는다. |
| G11 | [CodexBar: `docs/cursor.md`](https://github.com/steipete/CodexBar/blob/main/docs/cursor.md), [`CursorSandUsage.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorSandUsage.swift) | Cursor 계정의 `get-sand-usage-status`, quota percent·reset 및 `isUnified` 의미를 구현한 비교 사례. first-party xAI 인증 경로로 대체해서 설명하지 않는다. |

## Antigravity

| ID | 1차 자료 | 확인한 내용과 한계 |
| --- | --- | --- |
| A1 | [CodexBar: `docs/antigravity.md`](https://github.com/steipete/CodexBar/blob/main/docs/antigravity.md) | 로컬 language server, 원격 OAuth, CLI의 관계와 버전별 제약. 정상 인증 계정의 원격 API 403, CLI CSRF 변화 등 성공을 보장할 수 없는 조건을 포함한다. |
| A2 | [CodexBar: `AntigravityRemoteUsageFetcher.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Antigravity/AntigravityRemoteUsageFetcher.swift) | 직접 OAuth HTTP `retrieveUserQuotaSummary` 및 다른 API 대체 경로. 모델 availability와 측정된 quota의 구분. |
| A3 | [CodexBar: `AntigravityStatusProbe+LocalEndpoints.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Antigravity/AntigravityStatusProbe+LocalEndpoints.swift) | 로컬 endpoint 탐색·조회 및 CSRF를 포함한 요청 처리. 실행 중인 올바른 사용자·앱 인스턴스를 식별해야 한다. |
| A4 | [CodexBar: `AntigravityStatusProbe.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Antigravity/AntigravityStatusProbe.swift) | 경로 선택, CLI 버전·출력 및 실패 처리. 프로그램의 timeout은 제공자의 polling 제한이 아니다. |
| A5 | [CodexBar: `AntigravityCLISession.swift`](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Antigravity/AntigravityCLISession.swift) | CLI 세션 수명과 idle 종료의 구현 사례. 상시 실행·매번 실행의 실제 비용은 따로 측정해야 한다. |

## 프로젝트 비교에 참고한 자료

| 자료 | 사용 범위 |
| --- | --- |
| [Rfluid/aura](https://github.com/Rfluid/aura) | Rust + GPUI, macOS 및 여러 구독 제공자 조회를 조사한 선행 사례. source에 경로가 있다는 사실과 우리 계정에서의 성공을 구분한다. Gemini는 Light Usage Bar의 별도 provider 범위에 포함하지 않는다. |
| [afatmustafa/swapbar](https://github.com/afatmustafa/swapbar) | Rust + GPUI 기반 Claude 중심 macOS 앱의 비교 후보. 기능·메모리 우열을 측정 없이 단정하지 않는다. |
| [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar) | Rust 기반 macOS 사용량 표시 앱의 비교 후보. UI는 GPUI와 다르므로 동일한 구현이라고 분류하지 않는다. |
| [steipete/CodexBar](https://github.com/steipete/CodexBar) | quota source, 계정 경계, 실패 처리의 폭넓은 구현 참고와 macOS 비교 후보. 모든 기능을 재현하는 것은 초기 범위가 아니다. |

위 목록은 성능 순위가 아니다. 실행 파일·다운로드 크기, 앱 단독 RAM, 자식 프로세스를 포함한 RAM은 서로 다른 측정값이다. 비교하려면 같은 Mac·계정·제공자·갱신 조건에서 측정해야 하며 현재 그런 결과는 없다. 코드 재사용 시 각 저장소의 license를 별도로 확인한다.
