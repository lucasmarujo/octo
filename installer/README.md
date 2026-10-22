# Instalador do Windows

`orkai.iss` gera o `orkai-setup.exe` com o Inno Setup 6, o mesmo que vem nos runners Windows do GitHub. Ele
instala o orkai só para o usuário, em `%LOCALAPPDATA%\Programs\orkai`, sem pedir administrador, e a pasta
continua gravável para a atualização automática. A opção "Adicionar o orkai ao PATH" mexe só no `PATH` do
usuário (`HKCU\Environment`), sai na desinstalação e, desmarcada numa reinstalação, tira a pasta do `PATH`.

Rodar o instalador com o orkai aberto não fecha nada: o `orkai.exe` em uso é renomeado para
`orkai.exe.old-setup-*` antes da cópia, e o próprio orkai apaga esse arquivo na próxima abertura (o mesmo
prefixo `orkai.exe.old-` da atualização automática). Se a instalação falhar ou for cancelada, o binário
antigo volta para o lugar. A versão do instalador vem do `ProductVersion` do binário, que o `build.rs` grava a
partir do `Cargo.toml`.

## Build local

```powershell
cargo build --release
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" installer\orkai.iss
```

O instalador sai em `target\installer\orkai-setup.exe`. `/DBinary=<caminho>` troca o `orkai.exe` empacotado.

## Testes

```sh
cargo test --test installer_e2e -- --ignored
```

Esses testes instalam, atualizam com o orkai aberto e desinstalam de verdade para o usuário atual (registro do
Windows e `PATH`), por isso ficam fora do `cargo test`; o CI roda no runner Windows, que é descartável.
`ORKAI_ISCC` aponta para outro `ISCC.exe`. Sem `--ignored`, o `cargo test` só confere a versão gravada no
`orkai.exe`.

## Assinatura (SignPath Foundation)

Com a variável `SIGNPATH_ORGANIZATION_ID` definida no repositório, o job `build` do Windows manda dois pedidos
à SignPath: primeiro o `orkai.exe` (que vai dentro do instalador e é o binário da atualização automática),
depois o `orkai-setup.exe` gerado com ele. Cada pedido espera até 1 hora pela aprovação manual, que a SignPath
Foundation exige em toda release; se ninguém aprovar, o job falha e a release `latest` continua a anterior.
Depois da assinatura o CI confere as duas assinaturas com `Get-AuthenticodeSignature`. Sem a variável, a
release sai sem assinatura e o CI deixa um aviso.

Para ligar:

1. Candidatar o projeto em <https://signpath.org/apply>. A SignPath Foundation pede licença OSI (o repositório
   é MIT) e a seção "Code signing policy" no README e no site, que já estão no repositório.
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
