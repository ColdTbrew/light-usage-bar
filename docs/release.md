# 0.1.0 배포 후보 상태

갱신일은 2026-10-30이다. 이 문서는 배포 준비의 확인 결과다. 릴리스 발표문이 아니다. 공개된 릴리스 산출물은 아직 없다.

## 대상과 배포 방식

| 항목 | 현재 상태 |
| --- | --- |
| 버전 | `0.1.0` 후보 |
| CPU | Apple Silicon `arm64`. Intel 미지원 |
| 최소 OS | 패키지 설정은 macOS `26.0`. 실제 컴파일 확인 환경은 `26.6.2` |
| UI | AppKit 메뉴와 별도 GPUI `0.2.2` 패널 구현 |
| 패키지 | 이전 head `5bb0389`의 CI 번들 checksum, deep strict 서명과 잠금 해제 세션의 smoke 통과. 실제 화면과 조작 미확인 |
| 서명 | 기본 ad hoc. Developer ID 미확인 |
| 공증 | 미수행. 패키징 스크립트에 공증 단계 없음 |
| 배포 | 공개 업로드 전 |
| 라이선스 | 소유자 선택 전 |

## 확인한 근거

컴파일 환경은 M3 Pro MacBook Pro, RAM 18 GiB, macOS 26.6.2, Xcode 26.5다. `DEVELOPER_DIR`로 전체 Xcode를 선택했다. 기본 Metal toolchain을 다운로드한 뒤 GPUI를 포함한 워크스페이스 실행 파일 컴파일이 통과했다.

실계정 읽기 전용 조회는 Codex와 Grok Bot에서 성공했다. Codex에서는 7일 구간 하나를 확인했다. Grok에서는 서버 사용자 일치와 `bot.usage` 성공을 확인했다. Grok 응답에 사용률 필드가 없어 사용률은 `Unknown`으로 보존한다. 개인 사용량 숫자는 공개하지 않는다.

Claude는 잠금 해제한 Mac의 비대화형 probe에서도 Keychain `PermissionDenied`를 반환했다. 별도 진단은 credential 해석 전 Keychain 검색의 OSStatus `-25308`, `errSecInteractionNotAllowed`를 확인했다. 특정 ACL 원인이나 설치 앱의 신뢰 상태를 입증하지는 않는다. 사용량 조회 성공은 확인하지 않았다. 사용자 지정 Claude 프로필은 자체 인증 파일을 사용하며 전역 Keychain으로 넘어가지 않는다. Keychain helper에는 2초 제한과 취소 시 종료 및 회수 경로가 있다. 조건부 수동 승인 절차는 [Claude Keychain 접근 문제](providers.md#claude-keychain-접근-문제)에 있다.

Antigravity는 Google 서명과 같은 사용자를 확인한 로컬 language server를 탐색하고 읽기 전용 HTTPS로 조회한다. 설치 앱 2.24.0의 `GetUserStatus`에 email이 없어 `NeedsVerification`을 반환했다. 앱 종료 시 `TemporarilyUnavailable`도 확인했다. 실제 quota 조회 성공은 확인하지 않았다. 계정을 추정하거나 identity 확인을 생략하지 않는다. 연결 경계와 한계는 [제공자 참조](providers.md#antigravity-로컬-연결)에 있다.

## 아직 통과하지 않은 확인 항목

아래 항목은 미리보기 배포 전에 확인하거나 배포 제한으로 명시해야 한다. 체크하지 않은 항목을 통과로 해석하지 않는다. 전체 성능 비교를 미리보기의 조건으로 삼지는 않지만, 기존 MVP 성능 통과 조건은 그대로 남는다. 미리보기 패키지는 MVP 완료를 뜻하지 않는다.

- [x] 이전 head `5bb0389` CI 번들의 잠금 해제 세션 smoke에서 패널 시작, 첫 프레임 뒤 준비 응답, 종료와 회수 확인.
- [ ] 실제 화면 캡처로 패널 표시와 안내 문구 확인.
- [ ] 네이티브 메뉴와 패널에서 갱신, 활성화 변경, 인증 안내와 종료 조작.
- [ ] Escape, 바깥 클릭, 빠른 재개방, 패널 중복 실행과 자식 프로세스 정리.
- [ ] 복수 화면과 화면 배율, 키보드 조작, VoiceOver, 밝은 모드와 어두운 모드.
- [ ] sleep과 wake, offline, timeout, 인증 만료와 rate limit 시나리오.
- [x] 실제 release 앱 번들 생성, ad hoc 서명과 ZIP checksum 검증.
- [ ] 설치 후 실제 화면에서 실행.
- [ ] Developer ID 서명, 공증, stapling과 Gatekeeper 확인 또는 미리보기 배포 제한 명시.
- [ ] 공개할 릴리스 파일과 checksum 업로드 및 실제 다운로드 확인.
- [ ] 소유자의 라이선스 선택.

이전 head `5bb0389`의 CI 번들은 잠금 해제한 Mac에서 smoke 종료 코드 0과 남은 패널 프로세스 0개를 확인했다. 앞서 잠긴 화면에서 준비 응답을 확인하지 못한 결과를 이 실행으로 갱신했다. 첫 프레임 callback은 화면 캡처나 실제 버튼 조작을 입증하지 않는다. 사용자가 Mac을 조작할 수 없어 GUI 동의와 조작 검증은 연기했다. 제공자별 안내 문구를 바꾼 새 head의 실행과 화면 검증도 남아 있다.

Antigravity 변경을 포함한 macOS 워크스페이스 테스트 75개가 통과했다. macOS 전용 테스트의 초기화 문법을 수정한 뒤 제공자 테스트와 전체 대상의 엄격한 Clippy를 다시 통과했다. fmt, release 패키징, 최종 번들의 deep strict 서명 검사와 ZIP checksum 검사도 통과했다. 패키징 도구 테스트 4개의 이전 Mac 실행도 통과했다. 이 결과는 실제 quota 조회나 GUI 검증을 대신하지 않는다.

## 미리보기에서 설명할 제한

제공자별 조회 성공 범위는 [제공자 참조](providers.md)에 한정한다. 네 제공자 전체 지원을 광고하지 않는다. 사용량 정확성의 계정 화면 대조와 만료 상태 검증은 별도 작업이다.

기본 갱신은 600초이고 성공 값의 신선도는 120초다. 오류와 오래된 값을 구분한다. 공식 도구의 로그인 갱신, 자동 업데이트와 다중 계정 UI는 없다.

50회 패널 반복과 4시간 상주 실험, 다른 앱과의 비교는 후속 성능 검증 계획이다. RAM, CPU와 에너지 비교 결과는 아직 없다. 별도 패널 구조가 다른 앱이나 단일 프로세스보다 효율적이라는 주장은 하지 않는다. 재현 절차는 [성능 계획](performance.md), 결과는 [벤치마크 기록](benchmarks.md)에 둔다.

별도 단일 프로세스 prototype도 첫 프레임 smoke를 통과했다. 이는 production의 전체 기능 동등성이나 자원 사용 비교 결과가 아니다. helper 방식과의 같은 기능 비교, 50회 반복, 4시간 상주와 sleep 및 wake 검증은 남아 있다.

## 빌드 및 릴리스 자동화

`macOS application` workflow는 macOS 26 ARM64에서 테스트·Clippy·패키징을 수행하고 ZIP과 체크섬을 artifact로 올린다. [PR #2](https://github.com/ColdTbrew/light-usage-bar/pull/2)의 Antigravity 변경을 포함한 이전 head `5bb0389`, tree `a69fb6e`에서 [CI 실행 40905365316](https://github.com/ColdTbrew/light-usage-bar/actions/runs/40905365316)이 성공했다. 다운로드한 artifact `12912472512`의 checksum과 번들의 deep strict 서명을 확인했다. 이 결과는 해당 head에 한정한다. 제공자별 안내 문구와 문서를 바꾼 새 head의 CI, 패키징과 번들 검사는 아직 실행하지 않았다.

`Prepare draft release`는 main에서 수동 실행하며, 같은 커밋의 성공한 CI 실행 번호를 받는다. 원본 저장소, 커밋, workflow 경로, push 또는 수동 실행 여부와 체크섬을 확인한 뒤 미리보기 릴리스 초안을 생성한다. 이 릴리스 초안 생성 흐름의 실제 실행은 아직 확인하지 않았다.

앱 Resources에는 도달 가능한 의존성의 고지와 출처를 포함한다. 배포 crate에 빠진 고지는 upstream 선언, 배포 manifest와 참조 원문으로 보충하고 SHA-256으로 검증한다. 반복 패키징 때 오래된 리소스가 남지 않고 최종 복사본의 서명이 유효한 것도 Mac에서 확인했다. 이는 원문 고지 수집 결과이며 별도 라이선스 인증을 의미하지 않는다.
