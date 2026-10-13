# hsbg-overlay

Overlay de Hearthstone Battlegrounds para Linux/X11, feito para tiling WMs
como o **dwm**. Escrito em Rust; lê o `Power.log` do jogo (Wine/Proton) e,
só para as tribos da lobby, um campo da memória do cliente.

## O que mostra

| Painel | Onde | Conteúdo |
|---|---|---|
| Chances do combate | topo, centro | Letal / Vitória / Empate / Derrota / Letal, simuladas no início de cada combate (fica esmaecido na loja mostrando o último combate) |
| Turno + tiers | topo, direita | Turno atual e botões de tier 1–6/7 (o tier da sua taverna tem borda dourada) |
| Comps do meta | botão **Comps** no topo | Tier list de comps do HSReplay filtrada pelas tribos da lobby; clique abre o guia da comp |
| Lista de minions | direita | Clique num tier: minions agrupados por tribo + feitiços com custo. Hover mostra a carta, roda do mouse rola, botão direito fecha |
| Tribos + partidas | esquerda | Tribos da lobby e as últimas partidas com colocação e média da sessão |
| Tabuleiro do oponente | ao passar o mouse no leaderboard | Último tabuleiro visto daquele jogador (stats, palavras-chave, tier, há quantos turnos) |

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
  cada uma; hover mostra a carta), quando entrar na comp, enablers e como
  jogar. **‹** ou botão direito volta para a lista; botão direito na lista
  fecha.

Os guias são baixados em segundo plano e ficam em cache por 6 h
(`~/.cache/hsbg-overlay/comp_guides.json`); sem rede, o cache antigo é usado.

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

## Requisitos

- Rust (stable), Node.js ≥ 18 e npm
- Um compositor (picom, xcompmgr…) para painéis translúcidos; sem ele os
  painéis ficam opacos
- Log do Hearthstone ativado: o overlay cria/corrige o `log.config` do
  prefixo Wine automaticamente (reinicie o jogo se ele avisar)

## Instalação

```sh
./install.sh
```

Compila em modo release, instala `~/.local/bin/hsbg-overlay` e o simulador em
`~/.local/share/hsbg-overlay/sidecar` (`npm ci`). Para rodar direto do
repositório: `(cd sidecar && npm ci)` e `cargo run --release`.

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

## Ferramentas de diagnóstico

```sh
hsbg-overlay --replay <Power.log>            # cada combate: chances simuladas x resultado real
hsbg-overlay --render-ui <Power.log> <pasta>  # renderiza os painéis em PNG (inclui variantes do filtro), sem o jogo
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
  classes → campos) via `/proc/<pid>/mem`, numa thread que consulta a cada
  0,7 s.
- `src/comps.rs`: tier list e guias de comps de
  `hsreplay.net/api/v1/battlegrounds/comp_guides/` (público, sem login).
- `src/images.rs` + `src/net.rs`: imagens das cartas, baixadas e decodificadas
  em 4 threads de fundo, com cache em disco (`tiles/` e `renders/` em
  `~/.cache/hsbg-overlay`). As renders das cartas da tier list são pré-baixadas
  ao abrir o painel, então o hover já encontra o arquivo pronto.
- `src/x11.rs` + `src/ui`: janelas ARGB override-redirect desenhadas com
  tiny-skia/fontdue; posições relativas à janela do jogo, que é rastreada a
  cada 250 ms.

Testes: `cargo test` (parser, estado do jogo, tribos, filtros, leitor Mono
contra memória sintética).

Dados de cartas: export da Firestone (`static.zerotoheroes.com`, cache em
`~/.cache/hsbg-overlay`, atualizado a cada 24 h). Imagens: HearthstoneJSON.
O pacote do simulador é da Firestone e tem a licença dele; o `npm ci` o baixa
na sua máquina, ele não é redistribuído aqui.
