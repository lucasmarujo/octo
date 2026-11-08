# Instaladores do Windows

## Octo

`octo.iss` gera o `Octo-setup.exe` com o Inno Setup 6, o mesmo que vem nos runners Windows do GitHub. Ele
instala o Octo só para o usuário, em `%LOCALAPPDATA%\Programs\Octo`, sem pedir administrador, com atalho no Menu
Iniciar (e na área de trabalho, se marcado) e entrada em Aplicativos instalados. A pasta continua gravável para a
atualização automática do Octo.

Rodar o instalador com o Octo aberto não fecha nada: o `Octo.exe` em uso é renomeado para
`Octo.exe.old-setup-*` antes da cópia, e o próprio Octo apaga esse arquivo na próxima abertura (o mesmo prefixo
`Octo.exe.old-` da atualização automática). Se a instalação falhar ou for cancelada, o binário antigo volta para
o lugar. A versão e o copyright do instalador vêm do `ProductVersion` e do `LegalCopyright` do `Octo.exe`, que o
`tauri-build` grava a partir do `desktop/Cargo.toml` e do `desktop/tauri.conf.json` (`bundle.copyright`, e
`bundle.publisher` no `CompanyName`, igual ao `AppPublisher` do instalador).

### Build local

```powershell
cargo build --release -p orkai-desktop
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" installer\octo.iss
```

O instalador sai em `target\installer\Octo-setup.exe`. `/DBinary=<caminho>` troca o `Octo.exe` empacotado.

### Testes

```sh
cargo test -p orkai-desktop --test installer_e2e -- --ignored
```

Esses testes instalam, atualizam com o Octo rodando e desinstalam de verdade para o usuário atual (registro do
Windows e Menu Iniciar), por isso ficam fora do `cargo test` e tiram do lugar um Octo instalado pelo setup na
mesma máquina; o CI roda no runner Windows, que é descartável. `ORKAI_ISCC` aponta para outro `ISCC.exe`. Sem
`--ignored`, o `cargo test` só confere o nome e a versão gravados no `Octo.exe`.

### Assinatura

O `sign.ps1` assina um arquivo com o `signtool` do Windows SDK, usando um certificado de assinatura de código que
já está no repositório de certificados do usuário (`-Thumbprint`): digest SHA-256, carimbo de tempo RFC 3161
(`-TimestampUrl`, por padrão o da DigiCert) e descrição `Octo`. Depois confere que o arquivo saiu assinado por
esse certificado e com carimbo de tempo, e termina com erro se não saiu.

Com `/Socto=$p /DThumbprint=<impressão digital>`, o ISCC chama o `sign.ps1` para assinar o `Octo-setup.exe` e o
desinstalador; sem `Thumbprint`, o instalador sai sem assinatura, como antes:

```powershell
powershell -ExecutionPolicy Bypass -File installer\sign.ps1 -Thumbprint <impressão digital> -Path target\release\octo.exe
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" '/Socto=$p' /DThumbprint=<impressão digital> installer\octo.iss
```

No CI, o job `desktop-build` do Windows lê os segredos `WINDOWS_CERTIFICATE` (o `.pfx` em base64) e
`WINDOWS_CERTIFICATE_PASSWORD`, importa o certificado no runner, apaga o `.pfx`, assina o `Octo.exe` antes de
montar o instalador e de o `manifest.json` calcular os hashes, e monta o `Octo-setup.exe` assinado. Por último
confere os dois arquivos com `Get-AuthenticodeSignature`: cada um precisa estar `Valid`, assinado por esse
certificado e com carimbo de tempo; se não estiver, o job falha e a versão não é publicada. O resumo da execução
mostra a assinatura de cada arquivo. Sem os segredos, os dois saem sem assinatura e o CI deixa o aviso
`Octo is published unsigned`.

Um certificado OV ou IV emitido hoje não vem em `.pfx`: a chave fica num token ou num serviço na nuvem (como o
SimplySign da Certum). Nesse caso, o passo `Load the Windows signing certificate` troca a importação do `.pfx`
pelo login do serviço, que deixa o certificado no repositório do runner, e devolve a impressão digital dele; o
resto do fluxo não muda. Um certificado autoassinado assina, mas o Windows não confia nele e a conferência do CI
recusa.

`signs_the_app_and_the_installer_with_the_certificate_and_a_timestamp`,
`a_certificate_that_is_not_in_the_store_signs_nothing_and_stops_the_installer` e
`the_signed_installer_installs_a_signed_app_and_uninstaller` criam um certificado autoassinado descartável no
repositório do usuário, assinam com ele e o removem no fim; precisam da rede para o carimbo de tempo.

## Microsoft Store

`msix.ps1` gera o `Octo.msix` com o `makeappx` e o `makepri` do Windows SDK, a partir do `msix/AppxManifest.xml`
e dos ícones em `msix/Assets`. A Microsoft assina o pacote quando ele passa pela certificação da Store, por isso
ele sai daqui sem assinatura. A versão do pacote é o `ProductVersion` do `Octo.exe` com `.0` no fim, porque a
Store exige a quarta parte zerada.

```powershell
cargo build --release -p orkai-desktop
powershell -ExecutionPolicy Bypass -File installer\msix.ps1 -Binary target\release\octo.exe
```

O pacote sai em `target\installer\Octo.msix`, e a pasta montada fica em `target\msix`. A identidade já vem com os
valores da página "Product identity" do app no Partner Center (Store ID `9PCTM2ZPXW4J`): `OctoAI.OctoAIAgents`,
`CN=7C9FFDFE-038C-491B-903D-40B89EE3BD87`, publisher `OctoAI` e o nome reservado `Octo — AI Agents`, porque "Octo"
já era de outro publisher. A Store recusa um pacote com outros valores; `-Identity`, `-Publisher`,
`-PublisherDisplayName` e `-DisplayName` só servem para os testes.

No CI, o job `desktop-build` do Windows monta o `Octo.msix` com o mesmo `Octo.exe` da release e o deixa no
artefato `store-msix` da execução, fora da release do GitHub, porque um MSIX sem assinatura não instala fora da
Store. É esse arquivo que vai para a submissão no Partner Center.

O Octo da Store não se atualiza sozinho: a pasta do pacote é somente leitura e a própria Store instala as versões
novas. Ele mostra isso em Settings › Updates. O manifesto desliga a virtualização de escrita (`desktop6`, com a
capacidade restrita `unvirtualizedResources`), então o Octo grava no `%LOCALAPPDATA%\Octo` e no registro de
verdade, e o agents host e os holders, que rodam da cópia em `Octo\bin` fora do pacote e saem do job do Octo com
`CREATE_BREAKAWAY_FROM_JOB`, enxergam os mesmos arquivos e não têm a identidade do pacote. Por isso uma
atualização da Store nunca para um agente. Na submissão, a Store pede uma justificativa para `runFullTrust` e
`unvirtualizedResources`: o Octo abre agentes de linha de comando (Claude Code, Codex, npm…) que leem e gravam o
AppData e o registro reais do usuário e continuam rodando depois que o Octo fecha.

`installer/msix/Assets` sai do mesmo desenho do ícone do app (`site/favicon.svg`): os tamanhos de 16, 24, 32, 48 e
256 são idênticos aos do `desktop/icons/icon.ico`, e os logos de 150, 310 × 150 e 50 levam a janela de 144 ou 48
pixels com a mesma margem transparente de antes.

### Testes

```sh
cargo test -p orkai-desktop --test msix_e2e -- --ignored
```

Sem `--ignored`, o `cargo test` só confere o manifesto e os ícones; com ele, monta o pacote de verdade e precisa
do Windows SDK.

## orkai (legado)

Depois da 0.2.8 as releases trazem só o Octo: o CI não gera nem publica mais o `orkai.exe` e o
`orkai-setup.exe`, e o job `build` com a assinatura da SignPath saiu do workflow. O `orkai.iss` e os testes dele
ficam no repositório até o CLI sair; o que vem abaixo descreve como ele era publicado.

`orkai.iss` gera o `orkai-setup.exe` com o Inno Setup 6, o mesmo que vem nos runners Windows do GitHub. Ele
instala o orkai só para o usuário, em `%LOCALAPPDATA%\Programs\orkai`, sem pedir administrador, e a pasta
continua gravável para a atualização automática. A opção "Adicionar o orkai ao PATH" mexe só no `PATH` do
usuário (`HKCU\Environment`), sai na desinstalação e, desmarcada numa reinstalação, tira a pasta do `PATH`.

Rodar o instalador com o orkai aberto não fecha nada: o `orkai.exe` em uso é renomeado para
`orkai.exe.old-setup-*` antes da cópia, e o próprio orkai apaga esse arquivo na próxima abertura (o mesmo
prefixo `orkai.exe.old-` da atualização automática). Se a instalação falhar ou for cancelada, o binário
antigo volta para o lugar. A versão do instalador vem do `ProductVersion` do binário, que o `build.rs` grava a
partir do `Cargo.toml`.

### Build local

```powershell
cargo build --release
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" installer\orkai.iss
```

O instalador sai em `target\installer\orkai-setup.exe`. `/DBinary=<caminho>` troca o `orkai.exe` empacotado.

### Testes

```sh
cargo test --test installer_e2e -- --ignored
```

Esses testes instalam, atualizam com o orkai aberto e desinstalam de verdade para o usuário atual (registro do
Windows e `PATH`), por isso ficam fora do `cargo test`; o CI roda no runner Windows, que é descartável.
`ORKAI_ISCC` aponta para outro `ISCC.exe`. Sem `--ignored`, o `cargo test` só confere a versão gravada no
`orkai.exe`.

### winget

O CI só publica quando a versão sobe. Em cada push no `main`, o job `version` lê a `version` do `Cargo.toml` e
procura a release `v<versão>`: se ela já existe, o push só roda os testes; se não existe, os jobs `build` e
`publish` geram os binários e o instalador e criam a release, marcada como Latest no GitHub, e nunca mais mexem
nos arquivos dela. Não há mais build contínua (a tag `latest` saiu em 26/10/2026). Os links do README, do site e
do `llms.txt` usam `releases/latest/download/<arquivo>`, que o GitHub redireciona para a Latest, e a atualização
automática lê `releases/latest/download/manifest.json` e só instala quando a versão de lá é maior que a que está
rodando. Assim cada versão é o mesmo arquivo em todas as máquinas: a reputação no SmartScreen se acumula num hash
só, e o Defender vê um binário por versão em vez de um exe novo, sem assinatura, a cada push (em outubro de 2026
ele marcou um desses exes novos como `Trojan:Script/Wacatac.F!ml`). Para uma mudança chegar a quem usa o orkai,
suba a `version` do `Cargo.toml`.

Cópias do orkai de antes de 26/10/2026 que não abriram enquanto a tag `latest` existia ainda procuram por ela e
não se atualizam mais sozinhas; rodar o `orkai-setup.exe` uma vez resolve.

O pacote `LucasMarujo.orkai` do [winget-pkgs](https://github.com/microsoft/winget-pkgs) aponta para o
`orkai-setup.exe` de uma release versionada (`v0.1.0`, …), porque o winget confere o SHA-256 do instalador.
Instalado pelo winget, o SmartScreen não pergunta nada, porque o winget marca como confiável o instalador da
fonte oficial cujo hash bate com o manifesto; dali em diante o próprio orkai se atualiza a cada versão nova.

O winget roda o instalador com `/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART`, sem escolher tasks, e reconhece
o orkai instalado pelo nome, publisher e versão da entrada de desinstalação.
`installs_like_winget_on_the_path_under_the_entry_its_manifest_matches` instala assim e confere esses três
campos e o `PATH`; se `AppName` ou `AppPublisher` mudarem, o manifesto do winget muda junto.

Para publicar uma versão nova no winget: subir `version` no `Cargo.toml`, esperar o CI criar a release
`v<versão>` e rodar o [wingetcreate](https://github.com/microsoft/winget-create)
(`winget install Microsoft.WingetCreate`):

```powershell
wingetcreate update LucasMarujo.orkai --version <versão> --submit `
  --urls https://github.com/lucasmarujo/octo/releases/download/v<versão>/orkai-setup.exe
```

### Assinatura (SignPath Foundation)

Com a variável `SIGNPATH_ORGANIZATION_ID` definida no repositório, o job `build` do Windows manda dois pedidos
à SignPath: primeiro o `orkai.exe` (que vai dentro do instalador e é o binário da atualização automática),
depois o `orkai-setup.exe` gerado com ele. Cada pedido espera até 1 hora pela aprovação manual, que a SignPath
Foundation exige em toda release; se ninguém aprovar, o job falha, a versão não sai e o próximo push no `main`
tenta de novo.
Depois da assinatura o CI confere as duas assinaturas com `Get-AuthenticodeSignature`. Sem a variável, a
release sai sem assinatura e o CI deixa um aviso.

Para ligar:

1. Candidatar o projeto em <https://signpath.org/apply>. A SignPath Foundation pede licença OSI (o repositório
   é MIT) e a seção "Code signing policy" no README e no site, que já estão no repositório. A primeira
   candidatura, em outubro de 2026, foi recusada por falta de sinais públicos (estrelas, forks, contribuidores,
   menções fora do GitHub); a SignPath aceita uma nova quando o projeto tiver mais visibilidade. Aprovada, a
   seção "Code signing policy" dos dois READMEs volta a ter a atribuição que a SignPath pede.
2. Na SignPath: ligar o trusted build system **GitHub.com** ao projeto `orkai-cli`, instalar o SignPath GitHub
   App no repositório e criar as duas artifact configurations abaixo, com os slugs `orkai-exe` e
   `orkai-setup`. A signing policy usada é `release-signing`.
3. No GitHub, em Settings › Secrets and variables › Actions: a variável `SIGNPATH_ORGANIZATION_ID` e o secret
   `SIGNPATH_API_TOKEN`, de um usuário com permissão de submitter nessa policy.
4. MFA ligado na SignPath e no GitHub para todo mundo da equipe.

`orkai-exe`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" required="true" />
  </parameters>
  <zip-file>
    <pe-file path="orkai.exe" product-name="orkai" product-version="${version}">
      <authenticode-sign />
    </pe-file>
  </zip-file>
</artifact-configuration>
```

`orkai-setup`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" required="true" />
  </parameters>
  <zip-file>
    <pe-file path="orkai-setup.exe" product-name="orkai" product-version="${version}">
      <authenticode-sign />
    </pe-file>
  </zip-file>
</artifact-configuration>
```

O CI manda a versão do `Cargo.toml` no parâmetro `version`, então os dois arquivos de uma build têm o mesmo
`ProductVersion`, como a SignPath Foundation exige. O Inno Setup completa com espaços os campos de versão do
`orkai-setup.exe` (o Windows mostra `orkai` e `0.1.0`); se a SignPath recusar o pedido do instalador por causa
disso, a mensagem do pedido mostra o valor lido.
