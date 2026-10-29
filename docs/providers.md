# 제공자와 오류 참조

2026-10-29 기준이다. 파서 구현, 실제 계정 조회, 계정 화면과의 대조를 구분한다. 아래 결과는 관찰한 계정과 환경에 한정한다.

## 지원 상태

| 제공자 | 조회 경로 | 실제 확인 | 남은 제한 |
| --- | --- | --- | --- |
| Codex | `chatgpt.com/backend-api/wham/usage` | 읽기 전용 조회 성공. 7일 구간 하나 수신 | 다른 플랜과 조직, 만료 인증, 공식 계정 화면 대조 미완료 |
| Grok Bot | `computer-hub.grok.com/v1/tools?role=bot_client`의 `bot.usage` | OIDC 인증, 서버 사용자 일치, capability와 조회 성공 | 관찰 응답에 `usagePercent` 없음. 사용률은 `Unknown`. 기간과 초기화 시점은 보존. 팀 계정 미지원 |
| Claude | `api.anthropic.com/api/oauth/usage` | 비대화형 SSH에서 Keychain `PermissionDenied` | 실제 사용량 조회와 계정 화면 대조 미완료 |
| Antigravity | summary 응답 파서만 있음 | 실제 조회 없음 | PID, port, CSRF를 검증하는 로컬 서비스 연결 미구현. 항상 `NeedsVerification` |

Grok Bot은 xAI API 크레딧이나 일반 Grok 채팅 사용량을 뜻하지 않는다. Antigravity 앱은 관찰 환경에서 실행되지 않았다. 앱을 실행하면 현재 코드가 연결되는 것은 아니다.

이 경로는 제공자 내부 클라이언트와 기존 구현을 조사해 사용한다. 공식 외부 API의 장기 호환성 보장이 아니다. 조사한 revision은 [참고 자료](references.md)에 있다.

## 인증 정보 선택

앱은 공식 도구가 저장한 인증 정보를 읽는다. 로그인이나 토큰 갱신을 대신하지 않는다. 인증이 만료되면 공식 도구에서 다시 로그인한 뒤 앱에서 갱신한다.

| 제공자 | 기본 경로 | 선택 규칙 |
| --- | --- | --- |
| Codex | `~/.codex/auth.json` | `CODEX_HOME`을 지정하면 해당 디렉터리의 `auth.json`. ChatGPT OAuth만 허용. API key 결제 방식은 사용하지 않음 |
| Claude | `~/.claude/.credentials.json` | `CLAUDE_SECURESTORAGE_CONFIG_DIR`가 있으면 우선. 없으면 `CLAUDE_CONFIG_DIR`. 빈 값은 기본 디렉터리 의미 |
| Grok Bot | `~/.grok/auth.json` | `GROK_HOME`으로 디렉터리 변경. 공식 OIDC map에 후보가 여러 개면 `LIGHT_USAGE_BAR_GROK_SCOPE`로 선택 |
| Antigravity | 없음 | 인증 정보나 로컬 서비스 탐색을 아직 수행하지 않음 |

Claude 기본 프로필에서 파일이 없을 때만 macOS `Claude Code-credentials` Keychain 항목을 읽는다. 사용자 인증 UI가 필요한 항목은 건너뛰도록 요청한다. 항목이 여러 개면 임의로 선택하지 않는다. 명시한 사용자 프로필에서 파일이 없으면 전역 Keychain으로 넘어가지 않는다. Keychain helper의 제한 시간은 2초다.

Claude 내부 식별자는 인증 정보의 fingerprint다. 실제 사용자나 프로필의 확인 완료를 뜻하지 않는다. Codex는 선택 workspace와 사용자 claim을 검사한다. Grok은 서버의 사용자 ID가 선택한 인증과 일치하는지 확인한다. 조회 후 로컬 인증 정보를 다시 읽어 로그인 변경 중의 응답을 거부한다.

Finder로 실행하는 앱이 터미널의 환경 변수를 그대로 받는다고 가정하면 안 된다. 사용자 지정 경로가 필요하면 해당 변수를 지정한 셸에서 호스트 실행 파일을 실행한다. 다중 계정 선택 UI는 없다.

## 조회 제한

HTTP는 HTTPS만 허용하며 redirect를 따르지 않는다. 연결 제한은 5초, 네트워크 조회 제한은 15초, 응답 크기는 1 MiB다. 조회 후 인증 정보 재검사에는 별도 3초 제한이 있다. Grok WebSocket도 15초 제한과 1 MiB frame 및 message 제한, 수신 메시지 32개 제한을 사용한다.

Grok 조회는 handshake와 `bot.usage` 한 번으로 끝난다. 프롬프트, 컴퓨터 생성, 구독 변경 요청을 보내지 않는다. 공통 갱신 간격과 `Retry-After` 처리는 [아키텍처](architecture.md)에 있다.

## 표시와 오류

| 상태 | 의미와 대응 |
| --- | --- |
| `Unknown` | 필드가 없거나 의미를 검증할 수 없음. 0%가 아님 |
| `Missing` | 성공한 사용량이 없음 |
| `Stale` | 성공 값의 유효 시간이 지남. 마지막 조회 시각과 함께 확인 |
| `Refreshing` | 조회 중. 마지막 성공 값이 있으면 함께 표시 |
| `NeedsAuthentication` | 사용할 인증 없음 또는 만료. 공식 도구에서 로그인 후 수동 갱신 |
| `PermissionDenied` | 파일, Keychain 또는 서버가 접근을 거부. 재로그인만으로 해결된다고 단정하지 않음 |
| `RateLimited` | 서버 제한. 수동 갱신도 허용 시각까지 대기 |
| `TemporarilyUnavailable` | 통신 실패 또는 제한 시간 초과. 지연 후 재시도 |
| `Unsupported` | 현재 구현이 해당 계정이나 방식 지원 안 함 |
| `NeedsVerification` | 인증 선택이나 응답 의미를 확인하지 못했거나 조회 경로 미구현 |
| `InvalidResponse` | 응답이 예상 형식 또는 데이터 계약을 충족하지 않음 |

단위, 기간, 사용 방향과 잔여 방향을 그대로 보존한다. 구간 이름만으로 5시간이나 일주일을 추정하지 않는다. 구독 초기화 시점이 지났다는 이유로 새 값을 계산하지 않는다.

## 읽기 전용 진단

저장소 루트에서 실행한다.

```sh
cargo run --locked -p usage-providers --example probe -- codex
cargo run --locked -p usage-providers --example probe -- grok
cargo run --locked -p usage-providers --example probe -- claude
```

기본 출력은 계정 ID와 원문 응답 없이 상태와 구간 구조만 보여 준다. `--values`를 추가하면 사용량 숫자도 출력하므로 공개 로그에 사용하지 않는다. 지원 요청에는 [개인정보 설명](privacy.md)의 제외 항목을 확인한다.
