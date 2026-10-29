# 제공자와 오류 참조

2026-10-30 기준이다. 파서 구현, 실제 계정 조회, 계정 화면과의 대조를 구분한다. 아래 결과는 관찰한 계정과 환경에 한정한다.

## 지원 상태

| 제공자 | 조회 경로 | 실제 확인 | 남은 제한 |
| --- | --- | --- | --- |
| Codex | `chatgpt.com/backend-api/wham/usage` | 읽기 전용 조회 성공. 7일 구간 하나 수신 | 다른 플랜과 조직, 만료 인증, 공식 계정 화면 대조 미완료 |
| Grok Bot | `computer-hub.grok.com/v1/tools?role=bot_client`의 `bot.usage` | OIDC 인증, 서버 사용자 일치, capability와 조회 성공 | 관찰 응답에 `usagePercent` 없음. 사용률은 `Unknown`. 기간과 초기화 시점은 보존. 팀 계정 미지원 |
| Claude | `api.anthropic.com/api/oauth/usage` | 잠금 해제한 Mac의 비대화형 probe도 Keychain `PermissionDenied`. 별도 진단은 해석 전 OSStatus `-25308` | 실제 설치 앱의 접근 허용과 사용량 조회, 계정 화면 대조 미완료 |
| Antigravity | 실행 중인 앱의 loopback HTTPS `GetUserStatus`와 `RetrieveUserQuotaSummary` | 자체 listener의 macOS ABI 검사 통과. 설치 앱 2.24.0의 status 응답에 email이 없어 `NeedsVerification` | 확인된 standalone 설치 경로만 지원. identity를 보고하는 앱 상태에서 quota 조회와 계정 화면 대조 필요 |

Grok Bot은 xAI API 크레딧이나 일반 Grok 채팅 사용량을 뜻하지 않는다. Antigravity를 종료하면 새 사용량을 조회할 수 없다. 공식 앱을 열고 갱신한다. 로컬 관찰이 실패하면 이전 snapshot과 조회 시각을 보존하고 `Stale`로 표시한다. 계정이나 source 변경이 확인되면 이전 값을 지운다.

이 경로는 제공자 내부 클라이언트와 기존 구현을 조사해 사용한다. 공식 외부 API의 장기 호환성 보장이 아니다. 조사한 revision은 [참고 자료](references.md)에 있다.

## 인증 정보 선택

앱은 공식 도구가 저장한 인증 정보를 읽는다. 로그인이나 토큰 갱신을 대신하지 않는다. 인증이 만료되면 공식 도구에서 다시 로그인한 뒤 앱에서 갱신한다.

| 제공자 | 기본 경로 | 선택 규칙 |
| --- | --- | --- |
| Codex | `~/.codex/auth.json` | `CODEX_HOME`을 지정하면 해당 디렉터리의 `auth.json`. ChatGPT OAuth만 허용. API key 결제 방식은 사용하지 않음 |
| Claude | `~/.claude/.credentials.json` | `CLAUDE_SECURESTORAGE_CONFIG_DIR`가 있으면 우선. 없으면 `CLAUDE_CONFIG_DIR`. 빈 값은 기본 디렉터리 의미 |
| Grok Bot | `~/.grok/auth.json` | `GROK_HOME`으로 디렉터리 변경. 공식 OIDC map에 후보가 여러 개면 `LIGHT_USAGE_BAR_GROK_SCOPE`로 선택 |
| Antigravity | 실행 중인 앱의 language server | 같은 사용자, PID 시작 시각, 실행 경로, Google 서명과 소유한 TCP LISTEN socket을 확인. argv의 CSRF만 메모리에 보관 |

Claude 기본 프로필에서 파일이 없을 때만 macOS `Claude Code-credentials` Keychain 항목을 읽는다. 사용자 인증 UI가 필요한 항목은 건너뛰도록 요청한다. 항목이 여러 개면 임의로 선택하지 않는다. 명시한 사용자 프로필에서 파일이 없으면 전역 Keychain으로 넘어가지 않는다. Keychain helper의 제한 시간은 2초다.

Claude 내부 식별자는 인증 정보의 fingerprint다. 실제 사용자나 프로필의 확인 완료를 뜻하지 않는다. Codex는 선택 workspace와 사용자 claim을 검사한다. Grok은 서버의 사용자 ID가 선택한 인증과 일치하는지 확인한다. 조회 후 로컬 인증 정보를 다시 읽어 로그인 변경 중의 응답을 거부한다.

Finder로 실행하는 앱이 터미널의 환경 변수를 그대로 받는다고 가정하면 안 된다. 사용자 지정 경로가 필요하면 해당 변수를 지정한 셸에서 호스트 실행 파일을 실행한다. 다중 계정 선택 UI는 없다.

## Claude Keychain 접근 문제

먼저 Claude Code에서 로그인한 뒤 갱신한다. 사용자 지정 프로필은 해당 프로필의 `.credentials.json`을 사용한다. 전역 Keychain 권한을 바꿔도 사용자 지정 프로필의 누락 파일을 대신하지 않는다.

기본 프로필에서 Keychain 접근 권한이 필요한 경우에만 아래 절차를 적용한다. 해당 항목에 Access Control이 있어야 한다. [Apple의 앱별 Keychain 접근 설정](https://support.apple.com/en-gb/guide/mac-help/kychn002/26/mac/26)을 따른다.

1. Keychain Access에서 `Claude Code-credentials` 항목을 연다. 암호를 표시하거나 복사하지 않는다.
2. Access Control에서 기존 허용 앱을 유지하고, 실제 설치한 `/Applications/Light Usage Bar.app`을 추가한다. 임시 probe나 다른 빌드를 추가하지 않는다.
3. 변경 사항을 저장하고 macOS 인증 요청을 직접 승인한다.
4. 같은 설치 앱에서 Claude를 갱신하고 결과를 확인한다.

모든 앱에 대한 접근 허용은 선택하지 않는다. 인증 값이나 공식 도구의 파일은 편집하지 않는다. 항목이 없거나 여러 개이거나 Access Control이 없으면 임의로 항목을 만들거나 변경하지 않는다.

2026-10-30의 별도 진단은 credential 해석 전에 Keychain 검색에서 OSStatus `-25308`을 확인했다. Apple SDK의 이름은 `errSecInteractionNotAllowed`다. 이 결과만으로 특정 ACL이 원인이라고 단정하거나 최종 설치 앱의 신뢰 상태를 판단할 수 없다. probe와 설치 앱은 접근 권한을 확인해야 하는 실행 파일이 다르다. 설치 앱의 접근 성공과 실제 사용량 조회는 아직 확인하지 않았다. 사용자가 Mac을 조작할 수 없어 GUI 동의와 조작 검증은 연기했다.

## 조회 제한

HTTP는 HTTPS만 허용하며 redirect를 따르지 않는다. 연결 제한은 5초, 네트워크 조회 제한은 15초, 응답 크기는 1 MiB다. 조회 후 인증 정보 재검사에는 별도 3초 제한이 있다. Grok WebSocket도 15초 제한과 1 MiB frame 및 message 제한, 수신 메시지 32개 제한을 사용한다.

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
cargo run --locked -p usage-providers --example probe -- antigravity
```

기본 출력은 계정 ID와 원문 응답 없이 상태와 구간 구조만 보여 준다. `--values`를 추가하면 사용량 숫자도 출력하므로 공개 로그에 사용하지 않는다. 지원 요청에는 [개인정보 설명](privacy.md)의 제외 항목을 확인한다.
