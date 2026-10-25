# 제공자 연동 계획

**상태: docs-only.** Light Usage Bar는 macOS 전용 Rust + GPUI 상태 막대 앱을 계획하는 저장소다. 이 문서는 공개 소스에서 확인한 조회 방식과 구현할 동작을 구분한다. 앱 구현, 실제 계정 인증, quota 조회 및 성능 측정은 아직 수행하지 않았다.

**자료 확인일: 2026-10-25.** 대상은 **Claude, Codex, Grok Bot, Antigravity**다. 구독에 포함된 사용량과 초기화 시각을 보여주는 것이 목적이며, API 지출이나 로컬 토큰 수를 구독 quota로 대체하지 않는다. 근거 링크와 자료의 성격은 [출처 목록](references.md)에 정리한다.

## 확인 수준

| 표기 | 의미 |
| --- | --- |
| 소스 확인 | 공개 구현 또는 서비스 공식 문서에 해당 경로·필드가 존재한다. Light Usage Bar에서 동작한다는 뜻은 아니다. |
| 실계정 미검증 | 인증 수락 여부, 계정·플랜별 응답, 공식 UI와의 일치를 직접 검증하지 않았다. |
| 설계 제안 | 이 프로젝트에서 채택할 동작이다. 제공자가 보장한 API 계약이나 polling 제한이 아니다. |

기존 프로그램의 내부 API 사용 사례는 구현 근거다. 제공자가 외부 앱에 안정적인 공개 API를 제공한다고 해석하지 않는다. 문서의 `main` 링크는 변경될 수 있으므로 구현 착수 시 다시 확인하고 검증한 revision을 기록한다.

## 제공자별 우선 경로

| 제공자 | 소스에서 확인한 quota 경로 | 우선 구현 후보 | 남은 핵심 검증 |
| --- | --- | --- | --- |
| Claude | OAuth usage의 session·weekly·모델별 window | 유효한 기존 사용자 OAuth credential을 읽어 직접 HTTP 조회 | macOS Keychain 접근, 필요한 scope, 계정 일치, 만료 처리 |
| Codex | ChatGPT `wham/usage`의 rate limit window | 선택한 Codex home의 유효한 credential로 직접 HTTP 조회 | 계정·workspace 일치, credential 저장 방식, window 의미 |
| Grok Bot | 공식 Rust 프로토콜의 `bot.usage` | 인증을 검증한 뒤 짧은 WebSocket 연결로 조회 | **프로덕션 `bot_client`에서 기존 Grok OAuth credential이 수락되는지 미확인** |
| Antigravity | 실행 중인 로컬 서비스, 원격 OAuth HTTP, `agy /usage` | 기존 로컬 서비스 재사용; 가능하면 직접 HTTP | 앱이 닫힌 경우의 원격 API 접근, 버전별 CSRF, CLI 대체 경로 비용 |

## 공통 데이터 및 인증 원칙

### 값의 의미와 최신성

- snapshot에는 provider, 선택한 account/workspace의 내부 식별자, 실제 source, 측정 시각, quota window를 함께 둔다. 이전 계정의 결과가 새 계정에 표시되지 않도록 갱신 도중 선택이 바뀐 응답은 버린다.
- 각 window는 사용 비율 또는 잔여 비율의 의미, 기간, reset 시각을 보존한다. 응답에 기간이 없으면 임의로 `5시간` 또는 `주간`을 붙이지 않는다.
- 누락·권한 부족·파싱 실패는 **알 수 없음**이다. `0% 사용`, `100% 남음`, `무제한`으로 변환하지 않는다.
- 마지막 성공 값은 같은 계정에 한해 보존할 수 있다. 조회 실패 시 기존 측정 시각과 오류를 표시하며, 캐시를 읽은 시각을 새 측정 시각으로 바꾸지 않는다.
- quota, 유료 추가 사용, 선불 credits, API spend, 로컬 token estimate는 서로 다른 값이다. 초기 범위는 구독 quota이며 다른 지표로 빈칸을 채우지 않는다.
- `needs_auth`, `permission_denied`, `rate_limited`, `temporarily_unavailable`, `unsupported`와 같은 상태를 구분한다. 재시도 가능한 실패와 사용자의 조치가 필요한 실패를 동일한 무한 재시도로 처리하지 않는다.

### Credential 접근

기본은 사용자가 선택하고 접근을 허용한 기존 credential의 **읽기 전용 사용**이다. 다른 앱의 credential 파일, Keychain item, 계정 선택을 수정하지 않는다. OAuth refresh token의 회전과 저장을 누가 소유할지 별도로 설계하기 전에는 Light Usage Bar가 다른 앱의 refresh token을 갱신하거나 덮어쓰지 않는다.

macOS 권한이 필요한 최초 연결·복구는 사용자가 시작하는 흐름으로 둔다. 백그라운드 polling에서 로그인 창이나 Keychain 승인 창을 반복해서 열지 않는다. 만료된 credential은 기존 소유 앱에서 재인증하도록 안내하고, 실패를 숨기기 위해 다른 계정이나 다른 인증 경로로 자동 전환하지 않는다.

공개 저장소, issue, fixture, 로그에 실제 token, cookie, `auth.json`, Keychain payload, 계정 이메일을 넣지 않는다. HTTP header와 WebSocket 인증 정보는 로깅 전에 제거한다. 테스트 fixture는 합성 데이터를 사용한다. endpoint·필드 이름과 `~`로 표현한 일반적인 저장 위치는 구현 참고용이며 사용자 비밀값이 아니다.

### 조회 비용

정상 조회는 작은 네트워크 요청과 캐시 갱신으로 끝나는 방향을 우선한다. provider별로 실행 중인 요청을 합치고, 불필요한 CLI 시작·브라우저 실행·전체 대화 로그 스캔을 피한다. timeout, backoff, sleep/wake 동작과 캐시 정책은 구현 시 명시한다.

공개 자료만으로 네 제공자의 허용 polling 주기를 확정하지 않았다. 앱이 선택한 기본 주기를 공식 rate limit이라고 적지 않는다. `429`와 `Retry-After` 등 서버가 제공하는 신호를 존중하고, 초기화 countdown은 저장된 시각으로 계산한다.

## Claude

### 소스에서 확인한 내용

CodexBar는 Claude Code의 OAuth credential을 사용해 `GET https://api.anthropic.com/api/oauth/usage`를 조회한다. 현재 구현에는 reset 관련 선택 query와 호환성 처리가 추가되어 있다. 사용량 요청에는 Bearer 인증과 `anthropic-beta: oauth-2025-04-20` header를 사용하며, quota 접근에 `user:profile` scope가 필요하다고 문서화한다. 모델 호출용 token만 있거나 Keychain item에 MCP OAuth 상태만 있는 경우를 구독 사용량 credential로 취급하지 않는다. [C1–C2](references.md#claude)

일반적인 기존 저장소는 macOS의 `Claude Code-credentials` Keychain item과 `~/.claude/.credentials.json`이다. 어느 경로가 실제로 유효한지는 설치 버전과 사용자 설정에 따라 확인해야 한다. 다른 앱에서 로그인이 된 사실만으로 Light Usage Bar의 접근 권한이 확보되지는 않는다.

주요 응답은 `five_hour`, `seven_day`, 선택적인 모델별 window다. 사용 비율과 reset이 있는 window만 표시하고, 모델별 항목이 없다고 오류나 0%를 만들지 않는다. `extra_usage`의 금액은 quota 비율과 분리한다.

### 구현 제안

1. 사용자가 연결할 Claude 계정을 확인하고 허용한 credential source만 읽는다.
2. scope·만료를 확인한 유효한 access token으로 usage를 조회한다.
3. 계정 확인이 필요한 경우 profile 정보를 제한적으로 조회하고 같은 계정임을 확인한다.
4. 만료·scope 부족·Keychain 접근 거부는 해당 상태와 복구 방법을 표시한다.

브라우저 cookie 가져오기와 CLI PTY 자동화는 CodexBar에 존재하지만 초기의 필수 경로로 삼지 않는다. 먼저 직접 HTTP 경로의 동작과 유지 비용을 검증한다. 로컬 대화 기록의 token 수와 비용 추정은 Claude 구독의 남은 사용량을 증명하지 않는다.

### 완료 조건

실제 구독 계정에서 공식 Usage 화면과 session·weekly 값 및 reset 시각을 비교한다. scope 부족, 만료, Keychain 거부, 부분 window 응답, 네트워크 단절을 확인한다. 읽는 과정이 Claude Code의 credential을 변경하지 않는지도 검증한다. **현재 이 검증은 수행하지 않았다.**

## Codex

### 소스에서 확인한 내용

CodexBar는 `~/.codex/auth.json` 또는 선택한 `$CODEX_HOME/auth.json`의 OAuth credential로 `GET https://chatgpt.com/backend-api/wham/usage`를 조회한다. `rate_limit.primary_window`, `secondary_window`와 선택적인 `additional_rate_limits`가 표시 대상이다. 계정·workspace를 구분해야 하며 primary/secondary라는 필드 이름만으로 기간을 고정하지 않는다. [O1](references.md#codex)

현재 CodexBar 문서는 native `auth.json`에 갱신 token을 직접 쓰지 않고, stale credential의 갱신은 소유자인 Codex CLI에 맡기는 경계를 설명한다. `codex app-server` RPC도 대체 경로로 존재하지만, 선택한 workspace를 정확히 전달할 수 없는 경우 무조건 대체하지 않는다.

Aura의 Codex adapter에는 직접 `wham/usage` 조회와 로컬 `token_count.rate_limits` 기록을 사용하는 대체 경로가 있다. 로컬 기록은 **그 기록이 생성된 당시의 관측값**이다. 과거 snapshot을 현재 서버 quota라고 표시하거나, 누적 token 수에서 구독 quota를 추정해서는 안 된다. [O2](references.md#codex)

### 구현 제안

1. 선택한 Codex home과 계정·workspace에 조회를 고정한다. 다른 home을 자동으로 섞지 않는다.
2. 유효한 OAuth access credential을 읽고, 해당 계정에 필요한 request context를 보존해 직접 HTTP로 조회한다.
3. window별 사용 비율·기간·reset 및 추가 제한을 정규화한다. 없는 추가 제한은 생성하지 않는다.
4. 만료되면 Codex의 기존 로그인·갱신 흐름을 안내한다. 초기 버전은 credential을 직접 갱신하거나 계정을 교체하지 않는다.

API key만 설정된 계정은 ChatGPT 구독 quota credential과 구분한다. 선불 credits, API 비용, 로컬 token 기록을 session·weekly 사용률로 대체하지 않는다. 기존 CLI의 credential 저장 방식이 파일 이외인 경우는 별도로 검증하고, 파일이 없다는 이유만으로 구독이 없다고 결론 내리지 않는다.

### 완료 조건

실제 계정과 선택 workspace의 사용량을 공식 화면 또는 같은 계정의 CLI 상태와 비교한다. credential 회전 중의 파일 변경, 만료, 계정 전환 도중 완료된 응답, primary만 있는 응답, 추가 window를 검증한다. 로컬 snapshot 대체를 도입한다면 source와 원래 측정 시각을 표시한다. **현재 이 검증은 수행하지 않았다.**

## Grok Bot

### 범위와 구분

대상은 **xAI 구독에 포함된 Grok Bot 사용량**이다. xAI developer API의 선불 잔액·지출, Grok Build credits, 일반 Grok chat의 모델별 횟수 제한은 서로 다른 지표다. CodexBar의 `xai` 또는 `grok` provider가 있다는 사실만으로 Grok Bot quota를 지원한다고 판단하지 않는다. [G1–G3, G10](references.md#grok-bot)

공식 구독 문서는 새 X Subscription의 공유 주간 pool과 기존 linked 플랜을 구분한다. 실제 계정의 플랜과 quota 범위를 응답·공식 화면으로 확인한다. Cursor의 기존 Grok Bot 조회 구현은 비교 자료이며, **Cursor provider는 이 프로젝트의 지원 범위가 아니다**. Cursor 계정 데이터로 first-party xAI 연동이 완료된 것처럼 표시하지 않는다.

### 공식 소스에서 확인한 프로토콜

xAI 공식 `grok-build` 저장소에는 `wss://computer-hub.grok.com/v1/tools`와 `bot_client` handshake, `bot.usage` JSON-RPC 메서드가 있다. 이 메서드는 호출자 자신의 주간 사용량을 읽으며 클라우드 작업 머신을 깨우지 않는 조회로 설명된다. [G4–G6](references.md#grok-bot)

WebSocket upgrade 뒤 handshake의 형태는 다음과 같다.

```json
{"protocol_version":"1.0.0","kind":"bot_client"}
```

서버가 해당 기능을 지원하는지 확인한 뒤 보내는 요청의 형태는 다음과 같다. 이 예시는 프로토콜 참고이며 실행·인증 성공을 의미하지 않는다.

```json
{"jsonrpc":"2.0","id":1,"method":"bot.usage","params":{}}
```

| 필드 | 표시·파싱 원칙 |
| --- | --- |
| `usagePercent` | 없을 수 있다. 개인 meter가 없는 경우를 0%로 만들지 않는다. 100을 넘는 값도 보존한다. |
| `currentPeriodStartMs` | 응답에 근거한 기간 시작이며 로컬 계산으로 덮어쓰지 않는다. |
| `nextResetAtMs` | 첫 사용 전에는 없을 수 있다. 주간 reset인지 trial 만료인지 구분한다. |
| `trial`, `isTeamSeat` | 개인 유료 quota와 체험판·팀 플랜의 의미를 구분하는 근거로 사용한다. |
| `includedLimitZero`, `hasNonZeroIncludedLimit`, `hasAvailableUsage` | 포함 사용량·추가 사용 가능성의 상태다. 응답에 없는 비율을 역산하는 근거로 삼지 않는다. |
| `fundingPlan`, `planLabel`, `manageUrl` | 계정 플랜 및 관리 화면의 보조 정보다. API spend로 대체하지 않는다. |
| `onDemandEligible`, `onDemandEnabled` | 추가 사용 상태이며 포함 quota와 분리한다. |

### 인증은 아직 미검증

공식 SDK는 WebSocket upgrade의 Bearer 인증을 지원한다. 공식 workspace Hub 클라이언트는 `~/.grok/auth.json`의 OIDC credential을 읽고 갱신과 저장을 관리한다. **이 소스만으로 같은 credential이 프로덕션 `bot_client`의 `bot.usage`에서 수락된다고 확정할 수 없다.** [G7–G8](references.md#grok-bot)

CodexBar의 first-party Grok Bot 지원 요청 #4633은 확인일에 open 상태였다. 메서드의 존재와 별개로 전송 계층 지원 및 실제 credential 호환성 검증이 남아 있다. 미완성 통합을 완료된 연동 사례로 인용하지 않는다. [G9](references.md#grok-bot)

### 우선 수행할 검증과 구현 제안

1. 명시적으로 선택한 본인 계정의 기존 credential에 대해 읽기 전용 접근을 검증한다.
2. 인증된 handshake, 지원 capability, 계정 일치, 단일 `bot.usage` 응답을 확인한다.
3. 공식 사용량 화면과 값·플랜·reset 의미를 비교한다.
4. 성공한 경우에만 제한된 시간의 연결 → 조회 → 종료 및 캐시 갱신을 구현한다.

실제 인증이 검증되기 전에는 `unsupported` 또는 `needs_verification`으로 표시한다. API key나 Cursor 계정으로 조용히 전환하지 않는다. polling 허용 주기는 확인되지 않았다. 무작정 재접속하지 않으며, 장기 연결과 단발 조회 중 어느 방식이 적절한지는 서버 동작과 실제 자원 사용을 측정해 결정한다.

## Antigravity

### 소스에서 확인한 세 경로

CodexBar에는 실행 중인 앱의 로컬 language server, 원격 OAuth HTTP, CLI 조회가 구현되어 있다. 계정·클라이언트 버전에 따라 성공 경로가 다르므로, 모든 계정에서 CLI 없는 동작을 보장할 수 없다. [A1–A4](references.md#antigravity)

| 경로 | 확인한 구현 | 자원·동작 조건 |
| --- | --- | --- |
| 기존 로컬 서비스 | `LanguageServerService/RetrieveUserQuotaSummary`; 필요에 따라 다른 상태 메서드 | Antigravity가 이미 실행 중이며 같은 사용자의 대상 프로세스·포트·CSRF token을 확인할 수 있어야 한다. |
| 직접 OAuth HTTP | `POST https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary` | 유효한 계정 token과 필요한 project context가 있어도 일부 계정은 403을 받을 수 있다. |
| CLI | `agy -p /usage --output-format json` | CLI 버전·로그인 상태를 확인하고 실행 시간, 출력, 자식 프로세스를 제한해야 한다. |

로컬 서비스는 loopback HTTPS와 `X-Codeium-Csrf-Token`, `Connect-Protocol-Version` header를 사용한다. 서비스·계정 소유자를 확인하고 정상적으로 제공된 token을 사용한다. 새로운 `agy` 로컬 서비스는 token 없는 요청을 거부할 수 있으며, token을 얻을 수 없는 경우 이 경로가 된다고 가정하지 않는다. CSRF 검증을 우회하지 않는다.

직접 HTTP 경로는 내부 API 구현 사례다. 응답에 측정된 quota가 있는지 확인해야 한다. 모델 목록의 availability나 모두 100%인 기본값을 실제 잔여 quota로 해석하지 않는다. 선택한 계정의 OAuth client와 token 결합을 보존하며 임의의 client ID로 바꾸지 않는다.

CLI의 `/usage`는 상태 조회 경로지만 프로세스 시작·자동 업데이트·자식 프로세스 등의 비용을 따로 다뤄야 한다. CodexBar에는 자동 업데이트 억제와 자식 프로세스 정리, idle 시 CLI 세션 종료가 있다. 이를 참고하되 이 프로젝트에서 검증한 성능이나 서버 정책으로 기술하지 않는다. [A4–A5](references.md#antigravity)

### 구현 제안

1. 이미 실행 중인, 선택 계정의 로컬 서비스를 재사용할 수 있는지 확인한다.
2. 해당 계정에서 직접 OAuth HTTP가 실제 quota를 제공하는지 검증한다.
3. 두 경로가 불가능한 경우에만 CLI 대체를 검토한다. 자동 시작 여부·빈도·timeout을 명시하고 결과를 캐시한다.
4. 앱이 닫혔고 직접 HTTP도 거부되면 그 상태를 표시한다. 조회를 위해 Antigravity 전체 앱을 숨겨서 상시 실행하지 않는다.

같은 provider의 여러 경로를 동시에 호출해 비용을 중복시키지 않는다. 다른 계정으로 바뀌는 대체 경로는 허용하지 않는다. 원격 API 실패를 모든 모델의 quota 소진으로 표시하지 않는다.

### 완료 조건

앱 실행 중·종료 상태, 직접 API 403, token 만료, CLI 미설치, 현재 CLI의 CSRF 동작, CLI timeout과 자식 프로세스 정리를 확인한다. 모델별 quota와 reset을 공식 화면과 비교한다. CLI 경로가 필요하면 앱 프로세스와 자식 프로세스의 메모리를 함께 측정한다. **현재 이 검증은 수행하지 않았다.**

## 첫 구현의 검증 기록

구현 PR에는 source revision, 사용한 제공자 경로, CLI·앱 버전, 비식별 플랜 유형, 확인한 window, 측정 시각, 실패 동작을 기록한다. 비밀값과 원본 credential은 기록하지 않는다.

파서 검증은 합성 fixture로 시작한다. 실계정 검증은 읽기 전용 quota 조회로 제한하며, 검증을 위해 모델 요청·유료 추가 사용·구독 변경을 발생시키지 않는다. 소스에서 경로를 찾은 것, 우리 코드가 빌드된 것, 실계정 조회가 된 것, 자원 사용을 측정한 것은 별도의 완료 상태로 남긴다.
