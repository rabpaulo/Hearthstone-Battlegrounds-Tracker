# hsbg-overlay

Overlay de Hearthstone Battlegrounds para Linux/X11, feito para tiling WMs
como o **dwm**. Escrito em Rust; lê o `Power.log` do jogo (Wine/Proton) e,
só para as tribos da lobby, um campo da memória do cliente.

## O que mostra

| Painel | Onde | Conteúdo |
|---|---|---|
| Chances do combate | topo, centro | Letal / Vitória / Empate / Derrota / Letal, simuladas no início de cada combate (fica esmaecido na loja mostrando o último combate) |
| Turno + tiers | topo, direita | Turno atual, botão de ajustes e botões de tier 1–6/7 (o tier da sua taverna tem borda dourada) |
| Ajustes | botão de sliders no topo | Tema dos painéis: escuro, claro ou alto contraste. Troca na hora e grava a escolha no config |
| Comps do meta | botão **Comps** no topo | Tier list de comps do HSReplay filtrada pelas tribos da lobby; clique abre o guia da comp |
| Busca de cartas | botão **Search** no topo | Busca aproximada (fuzzy) por nome, filtrada por tipo de carta; hover ou setas mostram a carta |
| Lista de minions | direita | Clique num tier: minions agrupados por tribo + feitiços com custo. Hover mostra a carta, roda do mouse rola, botão direito fecha |
| Tribos + partidas | esquerda | Tribos da lobby e as últimas partidas com colocação e média da sessão |
| Tabuleiro do oponente | ao passar o mouse no leaderboard | Um botão por turno em que você enfrentou o jogador; clique para mostrar ou esconder o tabuleiro daquele turno (o mais recente vem ligado). Cada tabuleiro aparece na horizontal como no jogo: retrato do lacaio com ataque embaixo à esquerda e vida embaixo à direita, taunt, escudo divino, dourado e outras palavras-chave marcados, além do tier e de há quantos turnos; o popup continua aberto com o mouse sobre ele |

Não incluído: estatísticas exclusivas do HSReplay Premium (MMR e
estatísticas de heróis/comps) e o MMR do jogador.

### Comps do meta

O botão **Comps** na barra do topo abre a tier list de composições do
HSReplay (a mesma lista pública de guias que o HDT usa), agrupada em S/A/B/…
Cada comp mostra tribo, dificuldade e uma seta quando subiu ▲ ou caiu ▼ de
tier desde a última atualização.

- **Lobby only**: esconde as comps cuja tribo não está na lobby (desligado,
  elas aparecem esmaecidas com "not in lobby"). Comps mistas (ex.: Menagerie)
  sempre aparecem. Compartilha o estado com o toggle da lista de minions.
- Clique numa comp para abrir o guia: cartas core e add-ons (com o tier de
  cada uma; hover mostra a carta), heróis enablers, quando entrar na comp,
  enablers e como jogar. **‹** ou botão direito volta para a lista; botão
  direito na lista fecha.
- **Hero enablers**: heróis que terminam nessa comp muito mais vezes que os
  outros (ex.: Flurgl em Murlocs, Patches em Pirates, Ysera em Dragons), com
  quantas vezes mais (`5.2×`) e a colocação média do herói nela. Vêm das
  estatísticas públicas de comps da Firestone (último patch); a comp do
  HSReplay é casada com a da Firestone pelas cartas e, sem par, usa todas as
  comps da tribo. Com o seu herói entre os enablers, ele fica em dourado no
  guia e a comp mostra "your hero" na lista.

Os guias são baixados em segundo plano e ficam em cache por 6 h
(`~/.cache/hsbg-overlay/comp_guides.json`); sem rede, o cache antigo é usado.
O mesmo vale para os hero enablers: as estatísticas (~7 MB comprimidos) são
resumidas e só o resumo fica em `~/.cache/hsbg-overlay/hero_enablers.json`.

### Busca de cartas

O botão **Search** na barra do topo abre um campo de busca já pronto para
digitar. A busca é aproximada: aceita parte do nome (`bran`), abreviações
(`kalcgs`), palavras fora de ordem (`riven titus`) e erros de digitação
(`rivnedare`). A melhor carta fica destacada e aparece ao lado enquanto você
digita.

- **Tipo**: All, Minion, Spell, Hero, Trinket, Anomaly ou Reward. Com um tipo
  escolhido e o campo vazio, a lista mostra todas as cartas desse tipo.
- Teclado: **↑/↓** e **PgUp/PgDn** escolhem o resultado, **Tab** /
  **Shift+Tab** trocam o tipo, **Ctrl+W** / **Ctrl+Backspace** apagam uma
  palavra, **Ctrl+U** limpa, **Enter** devolve o teclado ao jogo (o painel
  fica aberto) e **Esc** fecha.
- Janelas de overlay não recebem foco no X11, então enquanto o campo está
  ativo o overlay prende o teclado (como o dmenu): o jogo e os atalhos do dwm
  não recebem teclas até **Enter**, **Esc** ou um clique fora do painel.
  Clique no campo para voltar a digitar. Botão direito fecha o painel.

### Lista de minions

- **Lobby only** (cabeçalho): mostra só as tribos da lobby; desligado, as
  tribos de fora aparecem esmaecidas com "not in lobby". O estado inicial vem
  de `hide_unavailable_tribes` no config.
- **Keyword**: clique na barra para abrir a grade e escolha uma keyword
  (Battlecry, Deathrattle, Divine Shield, Taunt, Reborn, Venomous, Windfury,
  Avenge, Rally, Start of Combat, End of Turn, Magnetic, Spellcraft, Choose
  One, Discover, Blood Gem). A lista passa a mostrar só minions e feitiços
  que têm a keyword ou interagem com ela (ex.: "dê Reborn"); grupos vazios
  somem. O filtro vale para todos os tiers até ser limpo no **×** ou em
  **Any**.

### Tribos da lobby

O jogo não escreve as tribos da lobby no log. O overlay as lê da memória do
cliente (`GameState.m_availableRacesInBattlegroundsExcludingAmalgam`, só
leitura — o mesmo que HDT/Firestone fazem), então elas aparecem já na escolha
de herói. Confira com `hsbg-overlay --lobby-tribes` durante uma partida.

A leitura precisa de acesso tipo ptrace ao processo do jogo:

- **Proton/umu (Lutris com GE-Proton, Steam):** funciona sem configuração — o
  contêiner do jogo roda num user namespace seu.
- **Wine puro** com `kernel.yama.ptrace_scope=1` (padrão do Arch): dê a
  capacidade só ao overlay, `sudo setcap cap_sys_ptrace=eip ~/.local/bin/hsbg-overlay`
  (refaça após reinstalar).

Sem acesso (ou com `read_game_memory = false`), as tribos são inferidas dos
minions que o Bob oferece, completas em 2–4 turnos; até lá a lista mostra as
não confirmadas com `?`.

### Temas e ajustes

O botão com ícone de controles deslizantes na barra do topo abre o painel
**Settings**:

- **Temas**: escolha entre **Dark** (tema escuro padrão), **Light** (tema claro
  para ambientes bem iluminados) e **High contrast** (fundo preto puro, bordas
  e textos nítidos; usa azul para vitórias e laranja para derrotas, pensado para
  daltonismo).
- **Troca ao vivo**: ao clicar em uma opção de tema, todos os painéis abertos
  são repintados imediatamente com a nova paleta.
- **Persistência automática**: a escolha é salva na hora em
  `~/.config/hsbg-overlay/config.toml`, preservando seus comentários, seções e
  demais chaves existentes.

## Requisitos

- Rust (stable, cargo), Node.js ≥ 18 e npm
- Um compositor (picom, xcompmgr…) para painéis translúcidos; sem ele os
  painéis ficam opacos
- Log do Hearthstone ativado: o overlay cria/corrige o `log.config` do
  prefixo Wine automaticamente (reinicie o jogo se ele avisar)

## Instalação

```sh
./install.sh
```

O script `./install.sh`:
- Valida os pré-requisitos (`cargo`, `node` ≥ 18, `npm`);
- Se for uma cópia clonada com upstream configurado e limpo, puxa novos commits automaticamente;
- Compila o binário em modo release (`cargo build --release`);
- Instala o executável em `~/.local/bin/hsbg-overlay` (apenas se houver mudanças);
- Copia e instala as dependências do simulador em `~/.local/share/hsbg-overlay/sidecar` (`npm ci`);
- Garante a criação da pasta de configurações `~/.config/hsbg-overlay`;
- Alerta caso `~/.local/bin` não esteja no seu `$PATH` ou se o overlay já estiver em execução.

Para desinstalar:
```sh
./uninstall.sh          # remove binário e pasta sidecar
./uninstall.sh --purge  # remove também ~/.config/hsbg-overlay e ~/.cache/hsbg-overlay
```

Para rodar direto do repositório em desenvolvimento:
```sh
(cd sidecar && npm ci)
cargo run --release
```

No dwm, inicie junto com a sessão (por exemplo no `~/.xinitrc`):

```sh
hsbg-overlay &
```

O overlay fica ocioso até achar uma partida de Battlegrounds. Os painéis são
janelas *override-redirect*: o dwm não as gerencia nem as coloca no tiling,
e elas somem quando você troca de tag ou foca outra janela.

## Configuração

`~/.config/hsbg-overlay/config.toml` — veja `config.example.toml`. Se o
popup do oponente não abrir no lugar certo, rode `hsbg-overlay --calibrate`
durante uma partida e ajuste `[leaderboard]`.

O tema dos painéis vem de `theme` (`"dark"`, `"light"` ou
`"high-contrast"`). Também dá para trocar ao vivo no botão de ajustes da
barra do topo: a escolha é aplicada na hora e reescrita no config, mantendo
comentários e as outras chaves. O alto contraste usa fundo preto e cores
vivas, com vitória em azul e derrota em laranja para quem tem daltonismo.

## Ferramentas de diagnóstico

```sh
hsbg-overlay --replay <Power.log>            # cada combate: chances simuladas x resultado real
hsbg-overlay --render-ui <Power.log> <pasta>  # renderiza os painéis em PNG (inclui variantes de filtro e settings), sem o jogo
hsbg-overlay --logs <pasta Logs>             # segue outra pasta de logs
hsbg-overlay --lobby-tribes                  # tribos da partida atual, lidas da memória
RUST_LOG=debug hsbg-overlay                  # log detalhado (inclui o simulador)
```

## Como funciona

- `src/logfile`: segue o `Power.log` mais recente (`Logs/Hearthstone_*/`) e
  interpreta o fluxo `PowerTaskList` (o que o cliente já animou).
- `src/game`: estado de entidades/tags; detecta turnos, leaderboard, ofertas
  da loja e tira um *snapshot* dos dois tabuleiros quando o bloco de preparação
  do combate fecha — antes de qualquer efeito de início de combate.
- `sidecar/sim.mjs`: processo Node com o simulador open-source da Firestone
  (`@firestone-hs/simulate-bgs-battle`); recebe o snapshot via JSON lines.
  Ele só sobe quando começa uma partida e é encerrado após 10 min sem jogo.
- `src/memory`: leitor mínimo do runtime Mono (domínio raiz → assemblies →
  classes → campos, incluindo suporte a instâncias de classes genéricas) via
  `/proc/<pid>/mem`, numa thread que consulta a cada 0,7 s.
- `src/comps.rs`: tier list e guias de comps de
  `hsreplay.net/api/v1/battlegrounds/comp_guides/` (público, sem login).
- `src/enablers.rs`: estatísticas e classificação de heróis enablers cruzando
  dados de composições do HSReplay com os rankings públicos da Firestone.
- `src/images.rs` + `src/net.rs`: imagens das cartas, baixadas e decodificadas
  em 8 threads de fundo, com cache em disco (`tiles/` e `renders/` em
  `~/.cache/hsbg-overlay`). As renders das cartas da tier list são pré-baixadas
  ao abrir o painel, então o hover já encontra o arquivo pronto. Se a render do
  modo Battlegrounds não existir (ex.: heróis), usa a render regular. As tiles
  de todas as cartas da busca são baixadas em segundo plano, só quando não há
  outro trabalho, para os resultados aparecerem sem download enquanto você
  digita.
- `src/cards.rs`: base de cartas do export da Firestone, com as categorias da
  busca (minion, feitiço, herói, trinket, anomalia, recompensa).
- `src/search.rs`: busca aproximada por nome (prefixo, palavras fora de ordem,
  abreviações e erros de digitação), usada pelo painel **Search**.
- `src/ui/theme.rs`: paletas de cores centralizadas (Dark, Light, High Contrast)
  usadas por todos os painéis, com persistência atômica no arquivo de configuração.
- `src/x11.rs` + `src/ui`: janelas ARGB override-redirect desenhadas com
  tiny-skia/fontdue; posições relativas à janela do jogo, que é rastreada a
  cada 250 ms.

Testes: `cargo test` (parser, estado do jogo, tribos, filtros, leitor Mono
contra memória sintética).

Dados de cartas: export da Firestone (`static.zerotoheroes.com`, cache em
`~/.cache/hsbg-overlay`, atualizado a cada 24 h). Imagens: HearthstoneJSON.
O pacote do simulador é da Firestone e tem a licença dele; o `npm ci` o baixa
na sua máquina, ele não é redistribuído aqui.
