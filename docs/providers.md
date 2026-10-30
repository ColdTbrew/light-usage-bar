# 제공자와 오류 참조

2026-10-30 기준이다. 파서 구현, 실제 계정 조회, 계정 화면과의 대조를 구분한다. 아래 결과는 관찰한 계정과 환경에 한정한다.

## 지원 상태

| 제공자 | 조회 경로 | 실제 확인 | 남은 제한 |
| --- | --- | --- | --- |
| Codex | `chatgpt.com/backend-api/wham/usage` | 읽기 전용 조회 성공. 7일 구간 하나 수신 | 다른 플랜과 조직, 만료 인증, 공식 계정 화면 대조 미완료 |
| Grok Bot | `computer-hub.grok.com/v1/tools?role=bot_client`의 `bot.usage` | OIDC 인증, 서버 사용자 일치, capability와 조회 성공 | 관찰 응답에 `usagePercent` 없음. 사용률은 `Unknown`. 기간과 초기화 시점은 보존. 팀 계정 미지원 |
| Claude | 공식 Claude Code CLI의 `/usage` 화면 | Mini GUI 세션의 production probe에서 두 구간의 측정값과 기간 수신 | 초기화 시점은 알 수 없음. 계정 바인딩 미확인. 공식 화면의 `5h`와 `7d` 수치 대조 미완료 |
| Antigravity | 실행 중인 앱의 loopback HTTPS `GetUserStatus`와 `RetrieveUserQuotaSummary` | 자체 listener의 macOS ABI 검사 통과. 설치 앱 2.24.0의 status 응답에 email이 없어 `NeedsVerification` | 확인된 standalone 설치 경로만 지원. identity를 보고하는 앱 상태에서 quota 조회와 계정 화면 대조 필요 |

Grok Bot은 xAI API 크레딧, 일반 Grok 채팅 사용량, CodexBar의 Grok credits와 구분한다. Antigravity를 종료하면 새 사용량을 조회할 수 없다. 공식 앱을 열고 갱신한다. 로컬 관찰이 실패하면 이전 snapshot과 조회 시각을 보존하고 `Stale`로 표시한다. 계정이나 source 변경이 확인되면 이전 값을 지운다.

이 경로는 제공자 내부 클라이언트와 기존 구현을 조사해 사용한다. 공식 외부 API의 장기 호환성 보장이 아니다. 조사한 revision은 [참고 자료](references.md)에 있다.

## 인증 정보 선택

Codex와 Grok Bot은 공식 도구가 저장한 인증 정보를 읽는다. 앱은 로그인이나 토큰 갱신을 대신하지 않는다. Claude는 공식 CLI를 실행하며 CLI가 자체 인증을 관리한다. 인증이 만료되면 공식 도구에서 다시 로그인한 뒤 앱에서 갱신한다.

| 제공자 | 기본 경로 | 선택 규칙 |
| --- | --- | --- |
| Codex | `~/.codex/auth.json` | `CODEX_HOME`을 지정하면 해당 디렉터리의 `auth.json`. ChatGPT OAuth만 허용. API key 결제 방식은 사용하지 않음 |
| Claude | 공식 `claude` 실행 파일과 실행 시 고정한 환경 | 앱이 Claude 인증 파일이나 Keychain 항목을 직접 읽지 않음. 프로필 해석과 로그인은 공식 CLI가 담당 |
| Grok Bot | `~/.grok/auth.json` | `GROK_HOME`으로 디렉터리 변경. 공식 OIDC map에 후보가 여러 개면 `LIGHT_USAGE_BAR_GROK_SCOPE`로 선택 |
| Antigravity | 실행 중인 앱의 language server | 같은 사용자, PID 시작 시각, 실행 경로, Google 서명과 소유한 TCP LISTEN socket을 확인. argv의 CSRF만 메모리에 보관 |

Claude는 수집마다 새 관찰 식별자를 사용한다. 이 식별자는 계정 ID나 인증 확인 결과가 아니다. **Details**에 `Account not identified for this reading`을 전달하는 동작은 단위 테스트로 확인했다. 새 앱 화면의 실제 문구 확인은 남아 있다. 출처 `ProviderReported`도 수치를 제공한 주체를 뜻하며 계정 확인을 뜻하지 않는다. 이전 관찰은 다음 조회 중 조회 시각과 함께 남을 수 있지만 현재 계정의 값으로 보장하지 않는다. 새 관찰은 이전 값을 대체하며 Claude 준비 실패 시 이전 값을 지운다.

Codex는 선택 workspace와 사용자 claim을 검사한다. Grok Bot은 서버의 사용자 ID가 선택한 인증과 일치하는지 확인한다. 두 제공자는 조회 후 로컬 인증 정보를 다시 읽어 로그인 변경 중의 응답을 거부한다. Claude의 source revision은 실행 파일과 프로필 범위 비교에만 사용하며 계정 연속성을 입증하지 않는다.

Finder로 실행하는 앱이 터미널의 환경 변수를 그대로 받는다고 가정하면 안 된다. 사용자 지정 경로가 필요하면 해당 변수를 지정한 셸에서 호스트 실행 파일을 실행한다. 다중 계정 선택 UI는 없다.

## Claude 로그인과 CLI 수집

1. 앱과 같은 사용자 및 프로필의 터미널에서 공식 Claude Code를 연다.
2. 공식 CLI에서 로그인을 마치고 `/usage`에 구독 사용량이 나오는지 확인한다.
3. Light Usage Bar에서 Claude를 수동 갱신한다.

로그인, 신뢰, 동의 또는 시작 승인 화면은 사용자가 공식 CLI에서 처리한다. 앱은 이 화면을 자동 승인하지 않는다. API billing이나 `Not logged in`만 보이면 구독 사용량 수집 성공으로 처리하지 않는다. `NeedsAuthentication`은 자동 재시도를 멈추며 로그인 후 수동 갱신할 수 있다. CLI 버전이나 필수 flag가 지원되지 않으면 `Unsupported`이며 제한을 푼 재실행은 없다.

앱은 `~/.local/bin/claude`, `/opt/homebrew/bin/claude`, `/usr/local/bin/claude` 순으로 실행 파일을 찾는다. 지원 범위는 major 버전 2의 `2.1.312` 이상이며 실행 전 `--help`에서 필요한 flag를 확인한다. 조회는 앱이 만든 사용자 전용 빈 임시 작업 디렉터리에서 PTY로 실행한다. 작업 디렉터리를 사용자의 프로젝트로 지정하지 않는다. 도구, hooks, MCP와 remote control을 사용하지 않도록 제한한다. 실행 인자는 `--allowed-tools '' --tools '' --mcp-config '{"mcpServers":{}}' --strict-mcp-config --settings '{"remoteControlAtStartup":false,"disableAllHooks":true}' --session-id <새 UUID>`다. `--help` 확인은 각 flag 이름의 지원 여부를 검사하며 모든 CLI 동작을 보증하지 않는다. 구현은 [`native.rs`](../crates/usage-providers/src/claude_cli/native.rs)에 있다.

버전과 도움말 확인, CLI 시작과 `/usage`는 하나의 45초 수집 예산을 공유한다. 정리에서는 종료 신호 뒤 250ms를 기다리고 강제 종료한 뒤 PTY를 닫고 직접 자식의 회수를 기다린다. 자식 회수 대기까지 45초 안에 끝난다는 보장은 아니다. 출력은 총 1 MiB로 제한한다. 수집 완료, 실패, 취소와 timeout에서 소유한 프로세스 그룹을 종료하고 직접 자식을 회수한다. Mac에서 네이티브 수명 주기 테스트 8개를 통과했다. 프로세스 그룹과 관찰한 자손의 부재, runtime 종료 전 취소 정리를 확인했다. 모든 형태의 이탈한 자손까지 회수함을 보장하지 않는다. CLI를 상주시키지 않는다. 준비 단계에서 완성한 관찰을 조회 단계가 소비하며 두 번째 CLI를 실행하지 않는다.

앱에는 Claude 인증 파일 직접 읽기, `Claude Code-credentials` Keychain helper 또는 OAuth HTTP fallback이 없다. Light Usage Bar를 Claude Keychain 허용 앱에 추가하는 절차도 사용하지 않는다. 공식 CLI는 자체 인증 저장소에 접근하거나 자체 상태를 변경할 수 있으며 시스템 인증 요청이 절대 없다고 보장하지 않는다.

2026-10-30 Mini GUI 세션에서 production probe의 준비와 조회가 성공했다. 측정값과 기간이 있는 두 구간을 수신했고 초기화 시점은 없었다. 진단 뒤 필터링한 프로세스 목록에 probe와 공식 CLI `2.1.312`가 남지 않았다. 검증한 probe의 SHA-256은 `08f65f4df3ab8b5d75db615c31d81c735a8fda6457b1e633415e205aea815bd1`이다. 개인 사용량과 계정 정보는 공개하지 않았다.

이 결과는 해당 GUI 세션과 프로필에 한정한다. 이전 SSH prototype의 인증 필요 결과와 GUI prototype의 성공 차이는 실행 환경에 따른 과거 관찰이다. 터미널 로그인만으로 GUI 앱이나 SSH 실행의 인증 상태를 보장하지 않는다. 새 앱의 사용자 제공 캡처에서 Claude의 `Up to date`, `5h`와 `7d` 숫자 및 막대, 알 수 없는 초기화 시점을 확인했다. Details와 버튼 조작, 공식 화면의 수치 대조는 남아 있다.

## 조회 제한

Codex와 Grok Bot의 원격 HTTP는 HTTPS만 허용하며 redirect를 따르지 않는다. 연결 제한은 5초, 네트워크 조회 제한은 15초, 응답 크기는 1 MiB다. 조회 후 인증 정보 재검사에는 별도 3초 제한이 있다. Grok WebSocket도 15초 제한과 1 MiB frame 및 message 제한, 수신 메시지 32개 제한을 사용한다.

Grok 조회는 handshake와 `bot.usage` 한 번으로 끝난다. 프롬프트, 컴퓨터 생성, 구독 변경 요청을 보내지 않는다. 공통 갱신 간격과 `Retry-After` 처리는 [아키텍처](architecture.md)에 있다.

## Antigravity 로컬 연결

지원 경로는 `/Applications/Antigravity.app/Contents/Resources/bin/language_server`다. `language_server` identifier와 Google TeamIdentifier `EQHXZ8M8AV`를 strict codesign 검사한다. 자동 실행, CLI 실행, OAuth, 인증 DB 읽기, 원격 fallback은 없다.

요청은 literal `127.0.0.1` HTTPS로 한정한다. 이 전용 client만 자체 서명 인증서를 허용하고 proxy와 redirect를 끈다. 원격 제공자의 TLS 검증은 유지한다. 프로세스의 IPv4 loopback 또는 wildcard LISTEN socket만 허용한다. IPv6 전용 listener는 현재 지원하지 않는다.

준비와 조회에는 각각 전체 15초 제한이 있다. 응답은 1 MiB, PID 목록은 32768개 미만, fd 목록은 4096개 미만, 후보 endpoint는 16개 이하로 제한한다. 후보 identity 하나라도 확인되지 않으면 첫 계정을 임의로 선택하지 않는다. 확인된 email이 서로 다르면 `NeedsVerification`이다.

조회는 준비한 endpoint에서 status, quota, status 순서로 실행한다. 각 요청 직전과 마지막 응답 뒤에 동일 process, token, socket을 재검사한다. email이나 source가 바뀌면 결과를 폐기한다. CSRF, argv, email 원문과 quota 응답은 로그에 남기지 않는다. `KERN_PROCARGS2`의 원시 버퍼에는 환경 바이트가 포함될 수 있지만 argc 뒤의 환경 항목은 해석하지 않는다.

설치 앱의 관찰된 `userStatus`에는 `cascadeModelConfigData`만 있었고 email이 없었다. 원인을 추정해 계정을 지정하거나 다른 저장소에서 계정을 읽지 않는다.

quota 응답에는 계정 ID가 없다. 앞뒤 status 검사는 중간에 A에서 B로 바뀌었다가 A로 돌아오는 전환을 입증하지 못한다. OS 재검사와 연결 사이의 짧은 경쟁 구간도 남는다. 자체 서명 서비스의 email은 서비스가 보고한 값이며 Google이 서명한 계정 증명이 아니다.

## 표시와 오류

| 상태 | 의미와 대응 |
| --- | --- |
| `Unknown` | 필드가 없거나 의미를 검증할 수 없음. 0%가 아님 |
| `Missing` | 성공한 사용량이 없음 |
| `Stale` | 성공 값의 유효 시간이 지남. 마지막 조회 시각과 함께 확인 |
| `Refreshing` | 조회 중. 마지막 성공 값이 있으면 함께 표시 |
| `NeedsAuthentication` | 사용할 인증 없음 또는 만료. 공식 도구에서 로그인 후 수동 갱신 |
| `PermissionDenied` | 파일 또는 서버가 접근을 거부. 재로그인만으로 해결된다고 단정하지 않음 |
| `RateLimited` | 서버 제한. 수동 갱신도 허용 시각까지 대기 |
| `TemporarilyUnavailable` | 통신 실패 또는 제한 시간 초과. 지연 후 재시도 |
| `Unsupported` | 현재 구현이 해당 계정이나 방식 지원 안 함 |
| `NeedsVerification` | 인증 선택이나 응답 의미를 확인하지 못했거나 조회 경로 미구현 |
| `InvalidResponse` | 응답이 예상 형식 또는 데이터 계약을 충족하지 않음 |

단위, 기간, 사용 방향과 잔여 방향을 그대로 보존한다. 구간 이름만으로 5시간이나 일주일을 추정하지 않는다. 구독 초기화 시점이 지났다는 이유로 새 값을 계산하지 않는다. Claude 화면의 날짜나 시간대가 명확하지 않으면 초기화 시점은 알 수 없는 값으로 남긴다. 상대 시각이나 추정 시각을 만들지 않는다.

### 할당량 구간 표시

Claude는 `five_hour`와 `seven_day` ID를 각각 `5h`와 `7d`로 표시하며 두 행을 항상 별도로 둔다. 구간이 없으면 `Usage unavailable`을 표시하고 막대나 초기화 시점을 만들지 않는다. 해당 계정에 할당량이 존재한다는 보장은 아니다. 모델별 주간 구간이나 기간만 같은 구간으로 계정 전체 `7d`를 채우지 않는다. 중복 구간과 추가 구간은 보존한다.

Codex의 모델 지정이 없는 `primary_window`와 `secondary_window`는 응답 기간이 18000초 또는 604800초일 때 각각 `5h` 또는 `7d`로 표시한다. primary라는 이름만으로 5시간을 추정하지 않는다. 다른 기간과 추가 구간은 원본 이름을 유지한다. **Details**에는 원본 ID, 이름과 기간을 보존한다. 사용량이 `Unknown`이면 0%나 비율 막대를 만들지 않는다.

Demo는 네 제공자의 합성 사용량과 초기화 시점 예시를 표시한다. 화면 안내와 **Details**의 출처는 `Synthetic demo data`다. 합성 값은 실제 계정 조회 성공이나 제공자의 할당량 계약을 입증하지 않는다.

이전 할당량 표시 후보 `bb810`의 Mac mini 일반 모드 캡처는 두 Claude 행과 다른 카드의 표시를 확인한 근거다. 새 CLI 수집기, 실제 구독 조회 또는 공식 계정 화면 대조의 근거가 아니다. Mini에는 Antigravity 앱이 없어 해당 환경의 quota 조회는 검증하지 않았다.

## 읽기 전용 진단

저장소 루트에서 실행한다.

```sh
cargo run --locked -p usage-providers --example probe -- codex
cargo run --locked -p usage-providers --example probe -- grok
cargo run --locked -p usage-providers --example probe -- claude
cargo run --locked -p usage-providers --example probe -- antigravity
```

기본 출력은 계정 ID와 원문 응답 없이 상태와 구간 구조만 보여 준다. `--values`를 추가하면 사용량 숫자도 출력하므로 공개 로그에 사용하지 않는다. 지원 요청에는 [개인정보 설명](privacy.md)의 제외 항목을 확인한다.
