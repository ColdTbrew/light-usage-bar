# 개인정보와 인증 정보

2026-10-30 기준 코드의 데이터 흐름을 설명한다. 외부 보안 감사나 제공자 인증을 받았다는 뜻은 아니다.

## 읽는 정보

호스트는 Codex와 Grok 공식 도구가 저장한 인증 정보를 읽고 해당 제공자의 사용량 서버에 인증 요청을 보낸다. Claude는 공식 CLI의 `/usage` 화면을 수집한다. 앱은 Claude 인증 파일이나 Keychain 항목을 직접 읽거나 OAuth HTTP fallback을 사용하지 않는다. 인증 정보의 저장 위치와 선택 규칙은 [제공자 참조](providers.md)에 있다.

Antigravity는 실행 중인 language server의 같은 사용자, 실행 경로, 시작 시각, Google 코드 서명과 LISTEN socket을 확인한다. 프로세스 인자의 CSRF와 로컬 status의 email을 메모리에 보관한다. 인증 DB나 다른 저장소에서 계정을 추정하지 않는다. 원시 프로세스 인자 버퍼에 환경 바이트가 포함될 수 있지만 환경 항목은 해석하지 않는다.

앱은 프롬프트와 대화를 수집하지 않는다. 앱 자체는 공식 도구의 인증 파일을 수정하거나 토큰을 갱신하지 않는다. Claude CLI는 자체 인증 저장소에 접근하고 자체 상태를 변경할 수 있다. CLI의 모든 쓰기나 시스템 인증 요청이 없다고 보장하지 않는다. 앱은 로그인 창이나 OAuth 흐름을 제공하지 않으며 로그인, 신뢰와 동의를 자동 승인하지 않는다. 인증이 만료되면 해당 공식 도구에서 로그인한다.

## 네트워크 목적지

| 목적지 | 목적 |
| --- | --- |
| `https://chatgpt.com/backend-api/wham/usage` | Codex 구독 사용량 조회 |
| 공식 Claude Code CLI가 선택하는 서비스 | CLI의 `/usage` 조회. 앱이 Claude OAuth endpoint를 직접 호출하지 않음 |
| `wss://computer-hub.grok.com/v1/tools?role=bot_client` | Grok Bot handshake와 `bot.usage` 조회 |
| `https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/GetUserStatus` | Antigravity 로컬 계정 확인 |
| `https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary` | Antigravity 로컬 quota 조회 |

제공자 조회에는 인증 토큰과 서버 요청에 필요한 계정 선택 정보가 들어간다. 앱 운영자의 서버, 분석 수집기나 원격 오류 보고 서비스는 구현하지 않았다. 앱 업데이트 확인 기능도 없다. 빌드 도구가 의존성을 받는 통신은 실행 중 제공자 조회와 별개다.

Antigravity 전용 client만 자체 서명 인증서를 허용한다. 목적지는 literal `127.0.0.1` HTTPS로 제한하며 proxy와 redirect를 사용하지 않는다. 원격 제공자의 TLS 검증은 유지한다. 요청은 status, quota, status 순서이며 계정이나 로컬 source 변경을 확인하면 결과를 폐기한다. email이 없으면 조회를 거절한다. 자체 서명 서비스의 email은 Google이 서명한 계정 증명이 아니다.

## 로컬 저장과 프로세스

사용량과 마지막 조회 오류는 호스트 메모리에 보관한다. 앱을 종료하면 사라진다. 현재 디스크 사용량 캐시는 없다. 제공자 활성화 여부 네 개는 아래 파일에 저장한다.

```text
~/Library/Application Support/Light Usage Bar/preferences.json
```

설정 파일에는 토큰과 계정 ID가 없다. 새 파일은 사용자 전용 권한 `0600`으로 작성한다. 임시 파일에 작성한 뒤 이름을 바꾼다.

Antigravity의 일시적인 관찰 실패는 메모리의 이전 값과 조회 시각을 보존하고 `Stale`로 표시한다. 계정이나 source 변경이 확인되면 이전 값을 지운다. 실제 설치 앱에서 quota 조회에 성공한 기록은 아직 없다.

GPUI 패널에는 사용량, 기간, 초기화 시점, 출처와 조회 상태만 전달한다. 계정 ID, workspace ID, 인증 정보와 원문 응답은 전달하지 않는다. 패널 통신은 앱이 만든 전용 pipe를 사용한다.

Claude CLI는 앱이 만든 사용자 전용 빈 임시 작업 디렉터리에서 실행한다. 도구, hooks, MCP와 remote control을 사용하지 않도록 제한한다. CLI 출력은 메모리에서 화면 해석에 사용하며 원문을 로그나 IPC로 내보내지 않는다. 수집에는 45초 예산을 두고 종료 정리 여유를 남긴다. 강제 종료 뒤 직접 자식의 회수를 기다리므로 총 소요 시간이 45초를 넘지 않는다는 보장은 아니다. Mac 네이티브 수명 주기 테스트 8개와 Mini GUI 세션의 production probe 종료 뒤 프로세스 확인을 통과했다. 모든 형태의 이탈한 자손 회수를 보장하지 않는다. 자세한 제한은 [Claude 로그인과 CLI 수집](providers.md#claude-로그인과-cli-수집)에 있다.

Claude의 관찰 식별자는 한 번의 수집을 구분하며 계정 확인을 뜻하지 않는다. 패널에는 식별자 대신 계정 미확인 상태를 전달한다. 이전 관찰과 현재 로그인 계정이 같다고 보장하지 않는다.

## 진단 자료 공유

일반 오류는 제공자와 제한된 오류 분류로 표시한다. 진단 예제는 기본값으로 사용량 숫자와 계정 ID를 출력하지 않는다. `--values`는 사용량 숫자를 출력한다. 의존성의 debug와 trace 로그는 빌드에서 비활성화해 WebSocket 인증 헤더와 원문 기록을 줄인다.

문제를 보고할 때 앱 버전, macOS 버전, CPU 아키텍처, 제공자 이름, 오류 상태와 재현 절차를 제공한다. 다음 정보는 공개 자료에서 제거한다.

- 토큰, 쿠키, Keychain 내용과 인증 파일.
- 이메일, 계정 ID, workspace ID와 개인 경로.
- 원문 HTTP 헤더, 응답 본문과 WebSocket 메시지.
- 프롬프트, 대화 내용과 공개를 원하지 않는 사용량 숫자.

앱을 삭제해도 공식 도구의 로그인은 해제되지 않는다. 앱 설정을 지우려면 앱 종료 후 위의 `Light Usage Bar` 설정 디렉터리를 삭제한다. 공식 도구의 인증 파일이나 Keychain 항목은 앱 삭제를 위해 지울 필요가 없다.
