# 제공자 adapter

`ProviderClient::prepare`는 매번 현재 credential을 읽는다. `PreparedProbe`는 비공개 credential과 `Identity`, 비교 전용 `CredentialRevision`을 묶는다. `fetch`는 응답 수신 후 credential을 다시 읽고 변경된 로그인에서 시작한 응답을 거부한다. 공식 client의 credential은 읽기만 한다.

호스트는 local prepare 실패 시 기존 선택과 snapshot을 지워야 한다. `identity()` 또는 `credential_revision()`이 바뀌어도 snapshot을 지운다. HTTP 오류와 timeout은 마지막 성공을 stale로 보존한다. Claude identity는 profile 확인을 주장하지 않는 credential fingerprint다. UI에서 계정 확인 완료로 표시하면 안 된다.

Codex는 `$CODEX_HOME/auth.json`, 기본 `~/.codex/auth.json`의 ChatGPT OAuth만 사용한다. Claude는 `$CLAUDE_CONFIG_DIR/.credentials.json`, 기본 `~/.claude/.credentials.json`을 읽는다. macOS에서 파일이 없으면 `Claude Code-credentials` Keychain을 사용자 상호작용 없이 읽는다. Grok은 `$GROK_HOME/auth.json`, 기본 `~/.grok/auth.json`의 공식 OIDC map을 읽는다. 후보가 둘 이상이면 `LIGHT_USAGE_BAR_GROK_SCOPE`로 scope를 명시해야 한다. Cursor credential은 읽지 않는다.

HTTP는 rustls, redirect 금지, 15초 timeout, 1 MiB 응답 제한을 사용한다. Grok WebSocket은 동일 deadline, frame 크기 제한, 최대 32개 메시지를 사용한다. `bot.usage` capability와 hub `user_id`를 검사한 뒤 단일 RPC를 호출한다. 실제 Grok production 계정 호환성은 미검증이다. Antigravity는 summary parser만 제공한다. same-user local service의 PID, port, CSRF 검증이 구현되지 않아 prepare는 `NeedsVerification`을 반환한다.

## 읽기 전용 진단

`cargo run -p usage-providers --example probe -- codex`는 계정과 응답 원문을 출력하지 않고 상태와 window 구조만 출력한다. `--values`를 명시하면 percent 숫자도 출력한다. 이 명령은 token을 갱신하거나 credential 파일을 쓰지 않는다.

## 근거

2026-10-29 조사 revision을 기준으로 구현했다. [OpenAI Codex b75d1e8](https://github.com/openai/codex/tree/b75d1e89665d256bf2c674b9590776f440c9bae7)의 passive WHAM reader와 auth storage, [xAI grok-build 2bdd1d6](https://github.com/xai-org/grok-build/tree/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8)의 handshake와 bot.usage, [CodexBar 5f93dc3](https://github.com/steipete/CodexBar/tree/5f93dc3068b3bee3f8753f3772614cc2949c335d)의 Claude OAuth와 Antigravity summary parser를 사용했다. Claude와 Antigravity 경로는 제공자의 공식 외부 API 지원 보장을 뜻하지 않는다.

## 구현 결정

Model the Domain 원칙에 따라 준비된 요청을 `PreparedProbe`로 묶었다. Boundary Discipline 원칙에 따라 credential 및 wire 검증을 이 crate에 가뒀다. Test Behavior, Not Implementation 원칙에 따라 테스트는 window 의미, 계정 불일치, 누락값, trial 만료를 검사한다.

Throughput checkpoint에서 첫 gate는 기존 `usage-core` 계약과 pinned 소스 확인이었다. 독립 작업은 부모의 host와 이 crate로 나눴다. 공유 mutable state는 별도 worktree로 분리했다. 이 crate는 한 작성자가 credential과 response identity 불변식을 함께 소유한다.

`ProbeFailure.identity_invalidated`가 true이면 조회 도중 local credential이 없어졌거나 바뀌었거나 서버 identity가 선택과 달랐다는 뜻이다. 호스트는 snapshot을 즉시 지운다. 일반 HTTP 401, 403, 429와 transient network 오류는 이 값을 false로 둔다. local credential 재검사는 별도 3초 제한을 사용하므로 fetch 전체 상한은 18초다.

`log` facade의 compile-time 최대 수준은 info다. 사용 중인 WebSocket 의존성이 trace에서 Authorization을 포함한 요청과 원문 frame을 기록하므로 debug와 trace를 build에서 제거한다. 이 설정은 host의 `log` 사용에도 적용된다.
