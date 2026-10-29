# ThirdPartyNotices 수집

패키징 담당자는 macOS 대상의 Cargo metadata를 저장한 뒤 새 출력 경로로 실행한다.

```sh
cargo metadata --locked --format-version 1 --filter-platform aarch64-apple-darwin > metadata.json
cargo run --locked -p notice-collector -- metadata.json ThirdPartyNotices packaging/licenses
```

선택적 세 번째 인자는 검토된 보충 고지 디렉터리다. `manifest.json`의 패키지 이름과 버전이 일치하고 원문 파일의 SHA-256이 일치할 때만 사용한다. 배포 crate에 고지가 없으면 정확한 upstream 커밋의 선언이나 배포 manifest와 참조 라이선스 원문을 보존한다. 출처 URL과 해시는 출력에 남기며 저작권자를 새로 추정하지 않는다.

첫 번째 인자에 `-`를 지정하면 stdin을 읽는다. 출력 디렉터리는 존재하면 안 된다. 실패 후 재실행할 때도 새 경로를 사용한다. 패키징 단계는 성공한 출력 디렉터리만 앱의 Resources로 복사한다.

`index.json`은 `usage-app`과 `usage-panel`에서 도달하는 registry 패키지의 이름, 버전, license expression, repository, source ID, 복사한 파일 경로를 기록한다. crates.io 패키지에는 해당 버전의 source archive URL과 source page URL도 기록한다. workspace 패키지는 제외한다. 입력 그래프의 build dependency와 development dependency도 포함하므로 실제 배포 바이너리보다 많은 패키지가 들어갈 수 있다. 수집기는 Cargo나 패키지 코드를 실행하지 않는다.

각 crate의 `LICENSE*`, `COPYING*`, `NOTICE*` 파일과 같은 접두사의 디렉터리 안에 있는 일반 파일을 대소문자 구분 없이 수집한다. metadata의 `license_file`도 수집한다. 파일 내용은 변환하지 않는다. 패키지와 파일 순서는 정렬되며 출력에 실행 시각이나 로컬 source 경로를 넣지 않는다.

종료 코드 `0`은 수집 과정에서 미해결 항목이 없음을 뜻한다. 종료 코드 `2`는 `index.json`의 `unresolved` 항목을 검토해야 함을 뜻한다. license 파일 누락, license expression 누락, symlink, registry 이외의 dependency를 미해결 항목으로 기록한다. 종료 코드 `1`은 잘못된 입력, 파일 입출력 실패 또는 제한 초과를 뜻한다. 이때 출력은 불완전할 수 있다. 코드 `2`에서 패키징을 중단하면 누락된 파일이 조용히 배포되는 일을 막지만, 사람이 별도로 확인해야 하는 패키지도 배포를 중단시킨다.

입력은 신뢰하는 Cargo 실행의 metadata여야 하며 수집 중 source tree가 바뀌면 안 된다. 수집기는 상대 license 경로의 상위 디렉터리 접근과 symlink를 거부하며 source tree 내부의 symlink를 따라가지 않는다. metadata는 64 MiB, 패키지는 10,000개, crate당 항목은 100,000개, 하위 디렉터리는 16단계로 제한한다. 파일은 각각 8 MiB, 전체 복사량은 512 MiB로 제한한다.

이 도구는 notice 파일 수집만 수행한다. license 선택, 배포 조건 판단, source 제공 의무 이행 여부는 판단하지 않는다. archive URL은 source 자체를 번들에 포함하지 않으며 해당 버전 repository commit도 보증하지 않는다. 별도 다운로드나 인증 정보 접근은 수행하지 않는다.

```sh
cargo test --locked -p notice-collector
```

검증 fixture는 중첩 notice 파일, 동일 입력의 출력 일치, 누락된 license, 상위 디렉터리 경로, 외부 파일과 디렉터리를 가리키는 symlink를 확인한다.
