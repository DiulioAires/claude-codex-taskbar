# Taskbar Usage Widget — Design

Data: 2026-10-30
Status: rascunho para aprovação

## 1. Intenção

Uma faixa compacta e bonita, fixa no **canto esquerdo da barra de tarefas do Windows 11**, que mostra de relance, sem abrir nenhum painel:

1. **Uso de hardware**, igual à aba *Desempenho* do Gerenciador de Tarefas: CPU, RAM, GPU e o "tempo ativo" de cada disco.
2. **Limites de assinatura** do **Claude Code** e do **Codex**: o percentual **usado** na janela de 5 horas e na janela semanal.

O projeto será distribuído como **open-source**. Qualquer pessoa deve poder baixar, rodar e ativar só o Claude, só o Codex, ambos ou nenhum.

O porquê: saber, sem interromper o trabalho, se o PC está sobrecarregado e quanto resta da cota antes de começar uma tarefa grande.

## 2. Objetivos

- Faixa sempre visível no canto esquerdo da barra, sem precisar abrir nada.
- **Leve de verdade**: um único `.exe` pequeno (alvo de poucos MB), sem runtime para instalar, consumo de RAM na casa de ~10–20 MB e CPU praticamente zero em repouso.
- **Standalone e fácil de instalar**: baixar e rodar. O próprio app oferece "Iniciar com o Windows".
- Cada item da faixa pode ser ligado ou desligado.
- Dados reais de limites, usando o login que o usuário já fez nas ferramentas oficiais (`claude` / `codex`).

## 3. Não-objetivos

- Não é um widget do painel de Widgets do Windows (Win+W). Esse painel exige abrir algo, e o usuário não quer isso.
- Não tem tela de login própria nem fluxo OAuth próprio.
- Não mostra espaço ocupado em disco, só atividade.
- Não guarda histórico nem gráficos ao longo do tempo (fora do escopo da v1).
- Não dá suporte a Windows 10 nem macOS/Linux.

## 4. Anti-objetivos (o que tornaria isso um fracasso)

- Ser pesado: executável grande, dependência de runtime, RAM ou CPU perceptível.
- Mostrar **números errados como se fossem certos**: token expirado ou endpoint quebrado devem aparecer como estado de erro claro, nunca como 0% ou como valor velho sem aviso.
- Atrapalhar o uso normal: ficar por cima de jogos e vídeos em tela cheia, cobrir ícones da barra ou "brigar" com o menu Iniciar.
- Corromper ou disputar as credenciais do Claude Code ou do Codex CLI.

## 5. Decisões tomadas

### 5.1 Pelo usuário

- **Local**: canto esquerdo da barra de tarefas, sempre visível.
- **Visual**: opção **B** do mockup. Para cada item: rótulo pequeno em cima, valor em destaque e uma barrinha horizontal de progresso embaixo. A barrinha muda de cor perto do limite.
- **Limites**: mostrar **% usado** de cada janela (5h e semanal), não o tempo até o reset.
- **Hardware**: sempre **uso**, como na aba Desempenho do Gerenciador de Tarefas, e nunca espaço ocupado.
- **Provedores**: o usuário ativa Claude e/ou Codex nas configurações.
- **Itens configuráveis**: cada item da faixa tem uma chave liga/desliga.
- **Interação**:
  - passar o mouse mostra um tooltip com detalhes: horário de reset de cada janela, horário da última atualização e mensagens de erro;
  - clique direito abre um menu com **Configurações**, **Atualizar agora**, **Iniciar com o Windows** (marcável) e **Sair**.
- **Prioridades**: standalone, fácil de instalar e **LEVE**.

### 5.2 Pelo Claude (decisões técnicas delegadas pelo usuário)

- **Linguagem**: Rust, usando as APIs Win32 diretamente (crate `windows`), sem framework de UI pesado. Desenho via Direct2D/DirectWrite (preferido, para texto nítido e cantos arredondados) ou GDI.
- **Janela**: popup sem bordas, `WS_EX_TOOLWINDOW` (não aparece em Alt+Tab nem na barra) e `WS_EX_TOPMOST`, com fundo transparente ou da cor da barra. Ela fica posicionada sobre a área livre à esquerda da barra, e não "dentro" da barra, porque o Windows 11 não permite mais *deskbands*.
- **Instalação**: `.exe` único em GitHub Releases. "Iniciar com o Windows" usa a chave `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Publicação no `winget` fica como passo posterior.
- **Configuração**: arquivo `config.toml` (ou JSON) em `%APPDATA%\<NomeDoApp>\`. A tela de Configurações pode ser uma janela Win32 simples com caixas de seleção.
- **Credenciais**: **somente leitura**. O app nunca renova tokens nem escreve nos arquivos das CLIs, para não competir com elas.

## 6. Comportamento detalhado

### 6.1 Itens da faixa (ordem padrão)

| Item | Rótulo | Fonte (Windows) |
|---|---|---|
| CPU | `CPU` | PDH `\Processor Information(_Total)\% Processor Utility`, que é o valor que o Gerenciador de Tarefas mostra. Fallback: `% Processor Time` |
| RAM | `RAM` | `GlobalMemoryStatusEx` (`dwMemoryLoad`, ou em uso/total) |
| GPU | `GPU` | PDH `\GPU Engine(*)\Utilization Percentage`: soma por engine e mostra o **maior** engine, como no Gerenciador |
| Disco (um item por disco físico) | `C:` / `D:` … | PDH `\PhysicalDisk(*)\% Idle Time` → tempo ativo = 100 − idle (limitado a 0–100). Rótulo com as letras do disco |
| *separador* | | |
| Claude 5h | `Claude 5h` | ver 6.2 |
| Claude semanal | `sem` | ver 6.2 |
| Codex 5h | `Codex 5h` | ver 6.3 |
| Codex semanal | `sem` | ver 6.3 |

- O hardware atualiza a cada **1 s** (padrão; configurável de 1 a 5 s).
- Cores das barras: hardware em azul (`#4cc2ff`), Claude em laranja (`#d97757`) e Codex em verde (`#10a37f`). Faixas de alerta: **≥ 75% amarelo** e **≥ 90% vermelho**. Os limiares são decisão do Claude e o builder pode ajustá-los.
- Mais de uma GPU: na v1, mostrar a de maior uso.
- Itens desligados somem, e a faixa encolhe.

### 6.2 Claude Code: limites

- Credenciais: `%USERPROFILE%\.claude\.credentials.json` → `claudeAiOauth.accessToken` e `expiresAt`.
- Consulta (endpoint **não oficial**, o mesmo usado pelo `/usage` do Claude Code): `GET https://api.anthropic.com/api/oauth/usage` com `Authorization: Bearer <token>` e `anthropic-beta: oauth-2025-04-20`. Resposta esperada: `five_hour.utilization`, `five_hour.resets_at`, `seven_day.utilization` e `seven_day.resets_at`. **O builder deve verificar o formato real** com uma chamada antes de implementar.
- Frequência: a cada **2 minutos** (configurável), mais "Atualizar agora".
- Token com `expiresAt` no passado ou resposta 401: mostrar o estado **"expirado"**, com a barra cinza e o valor `—`. O tooltip diz "Login do Claude expirado — abra o Claude Code para renovar". Não tentar renovar.
- Arquivo ausente: estado **"sem login"**, com o tooltip "Faça login rodando `claude`".

### 6.3 Codex: limites

- Credenciais: `%USERPROFILE%\.codex\auth.json` → `tokens.access_token` e `tokens.account_id`. Também respeitar `CODEX_HOME`, se estiver definido.
- Consulta (endpoint **não oficial**, o mesmo usado pelo `/status` do Codex CLI): `GET https://chatgpt.com/backend-api/wham/usage` com `Authorization: Bearer <token>` e `ChatGPT-Account-Id: <account_id>`. Resposta esperada: `rate_limit.primary_window` (5h) e `rate_limit.secondary_window` (semanal), cada uma com `used_percent` e o horário de reset. **O builder deve verificar o formato real.**
- Fallback, caso a consulta falhe: ler o evento mais recente com `rate_limits` nos arquivos `~/.codex/sessions/**/*.jsonl`, marcando no tooltip "dado de HH:MM (última sessão)".
- Mesmas regras de frequência e de estados de erro do Claude ("expirado", "sem login": "Faça login rodando `codex login`").
- Login por API key, sem plano ChatGPT: não há limites de assinatura. Mostrar o estado "sem assinatura", com o tooltip explicando.

### 6.4 Estados de erro (válido para os dois provedores)

| Estado | Valor | Barra | Tooltip |
|---|---|---|---|
| OK | `62%` | cor do provedor e alerta | reset 5h às HH:MM, reset semanal dia/HH:MM, atualizado às HH:MM |
| Dado velho (falhou, mas há valor recente com menos de 15 min) | valor | cor esmaecida | "falha ao atualizar; último dado às HH:MM" |
| Expirado / sem login / sem assinatura / erro | `—` | cinza | motivo e ação sugerida |

Nunca mostrar 0% no lugar de erro.

### 6.5 Posicionamento e convivência com a barra

- Ancorar no canto esquerdo da barra do **monitor principal**, centralizada verticalmente e com margem lateral de ~8 px.
- Altura: até a altura útil da barra (~48 px no padrão). Seguir a escala de DPI do monitor (per-monitor DPI aware v2).
- Barra alinhada à esquerda (ícones começam no canto): a faixa colide com os ícones. Na v1, detectar o alinhamento (`TaskbarAl` no registro) e, se estiver à esquerda, mostrar um aviso no tooltip ou nas configurações sugerindo centralizar a barra. A faixa não se reposiciona.
- Botão de Widgets do Windows ativo: ele ocupa o mesmo canto. Nas configurações, mostrar uma dica para desativá-lo (Configurações → Personalização → Barra de tarefas → Widgets).
- **Tela cheia**: esconder a faixa quando o app em primeiro plano estiver em tela cheia no mesmo monitor (`SHQueryUserNotificationState` ou comparação do retângulo da janela em foco com o monitor) e mostrá-la de novo ao sair.
- **Barra auto-ocultável**: a faixa acompanha a barra, escondendo e reaparecendo com ela (via `SHAppBarMessage` / posição da janela `Shell_TrayWnd`).
- **Manter por cima**: reafirmar o z-order (`SetWindowPos HWND_TOPMOST`) quando a barra é ativada e também periodicamente (por exemplo, a cada 2 s), porque o Windows 11 às vezes coloca a barra por cima.
- Reagir à mudança de resolução, DPI ou explorer reiniciado (`TaskbarCreated`) reposicionando a faixa.
- Tema: seguir o modo claro ou escuro do Windows (`AppsUseLightTheme` / `SystemUsesLightTheme`) nas cores do texto.

### 6.6 Configurações

Janela simples com:
- chaves por item: CPU, RAM, GPU, cada disco detectado, Claude 5h, Claude semanal, Codex 5h e Codex semanal;
- chaves por provedor: Claude e Codex (desligar o provedor oculta os dois itens dele e para as consultas). Ao ligar, mostrar o status: "login encontrado", "não encontrado — rode `claude`" etc.;
- intervalo de atualização do hardware e dos limites;
- Iniciar com o Windows.

Padrão na primeira execução: todo o hardware ligado. Provedores ligados automaticamente se o arquivo de login existir; caso contrário, desligados.

### 6.7 Decisões do Claude após a revisão (o usuário pode mudar)

- **Faixa maior que o espaço livre**: medir o espaço até o início dos ícones da barra. Se não der para medir, usar a metade esquerda da barra menos a largura estimada dos ícones. Se a faixa não couber: (1) trocar para o modo compacto, sem a linha de rótulo, só valor e barrinha; (2) se ainda não couber, ocultar os itens do fim para o começo e mostrar um "…". Os itens ocultos aparecem no tooltip. Nunca cobrir os ícones.
- **Fallback do Codex (arquivos de sessão)**: usar o dado só se o reset daquela janela ainda não passou, marcando no tooltip "dado de HH:MM". Se o reset já passou, mostrar `—` ("sem dado recente").
- **Idioma da interface**: inglês e português desde a v1, seguindo o idioma do Windows (inglês como padrão). Todos os textos ficam numa tabela de strings, para facilitar traduções da comunidade. Os textos em português deste documento são a referência para o pt-BR.
- **Assinatura de código**: a v1 sai sem assinatura. O README terá uma seção explicando o aviso do SmartScreen e como verificar o hash SHA-256 publicado no release. Avaliar assinatura gratuita para open-source (ex.: SignPath Foundation) depois.

## 7. Restrições

- Windows 11 (x64; ARM64 desejável, mas não obrigatório na v1).
- Sem dependência de runtime (.NET, VC++ redistribuível, WebView2 etc.). Linkar o CRT estaticamente.
- Sem privilégios de administrador.
- Nenhuma telemetria. Rede só para os dois endpoints de limites.
- Credenciais nunca são logadas, copiadas nem enviadas a outro lugar.
- Licença open-source: MIT, decisão do Claude e alterável pelo usuário.
- O projeto não é afiliado à Anthropic nem à OpenAI. O README deve deixar claro que os endpoints de limites não são oficiais e podem quebrar.

## 8. Deixado ao builder

- Nome do app e do executável.
- Escolha exata entre Direct2D e GDI, crates de HTTP (preferir WinHTTP nativo ou um cliente pequeno como `ureq` com rustls) e JSON (`serde_json`).
- Fonte e tamanhos exatos (Segoe UI Variable ou Segoe UI), espaçamentos e arredondamentos, mantendo o visual do mockup B.
- Estrutura do código e estratégia de testes (parsers de credenciais e de respostas devem ter testes unitários com fixtures).
- Pipeline de release (GitHub Actions gerando o `.exe`).
