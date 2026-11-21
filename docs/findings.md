# Levantamento inicial (2026-11-21)

Tags: **[fato]** verificado aqui (comando rodado ou arquivo lido), **[HXtra]** afirmado na pesquisa do HXtra (`docs/research`, tags deles entre parênteses), **[hipótese]** não verificado.

## Arquivos de origem (somente leitura, em `firmware-originals/`, fora do git)

| arquivo | MD5 |
|---|---|
| `HX_Stomp_v3.80.0_0x103966115.hxf` | `a2cd8f4ae56f49415380270ebc484bf1` (igual ao esperado pelo HXtra) |
| `HXtra_v1.2.1.hxf` | `f32a8f039ed446470292a042c82920d8` |
| `HX Stomp Backup 2026-Nov-14.hxb` | `ad8ad72475e872a379673d53d7e84af1` (antes do HXtra) |
| `HX Stomp Backup 2026-Nov-19.hxb` | `8dec32fecf1b8f901192d0e74f339cda` (com HXtra) |

## USB no Windows

- [fato] HX Stomp = `0E41:4246`, dispositivo composto (`usbccgp`). Interface 0 (controle/editor) usa **WinUSB** (INF da Line 6, `oem71.inf` v1.0.0.8); interface 1 é áudio (`l6hxstomp`); interface 5 é HID (teclado).
- [fato] libusb (pyusb + `libusb-package`) abre a interface 0 sem trocar driver: `hx_ctl.py status`, `presets` e `get-preset` funcionaram.
- [HXtra] O HX Edit segura a interface 0: o HX Edit e o nosso app não usam o USB ao mesmo tempo.

## Protocolo

- [HXtra, verificado no aparelho] Transporte, streams e MessagePack estão documentados em `tools/hx_dfu_reboot.py` e `tools/hx_ctl.py`. Três streams: editor (svc 6), librarian (svc 2), notify (svc 4). Comandos conhecidos: `0x14` carregar preset, `0x15` gravar edit buffer (sem salvar), `0x16` ler edit buffer, `0x17` preset atual, `0x2b` mover bloco; librarian `0x01` listar presets.
- [HXtra] O stream notify precisa ser lido; se não for lido, o aparelho trava depois de cerca de 30 escritas.
- [HXtra] 110 dos 124 comandos do editor estão registrados no firmware. A maior parte (salvar, renomear, baixar e subir preset, IR, globais) ainda não está documentada.
- [fato] O OpenHX (rusb) reproduz sequências de bytes capturadas no Stomp XL/Helix. O HXtra entende o protocolo de forma mais geral.

## Catálogo de modelos (do .hxf do usuário)

- [fato] A parte 3 do firmware tem **864 modelos** (stock) com nome interno, `displayName`, categoria/subcategoria e parâmetros: nome, tipo (1 bool, 2 int/lista, 3 float, 6 ?), slot, mínimo, máximo, padrão, `displayType` (knob, db, percent, time, list...), unidade, rótulos de listas, nomes curtos e meta de sync de tempo.
- [fato] Nos parâmetros de tipo 2, mínimo/máximo/padrão são inteiros guardados em bits de float (por isso aparecem como 1e-45).
- [fato] O ID que o preset guarda (bloco, chave 25) é o `preset_id` da Table 1 da parte 0, **não** o `model_id` da parte 3. O mapa nome→`preset_id` sai da Table 1 (823 linhas no stock, 824 no HXtra). Conferido contra o edit buffer real (21 = Essex A30, 124 = Deluxe Comp Stereo, 555 = NAM A2 Lite).
- [fato] Os nomes de parâmetro da parte 3 (`ChVol`, `BiasX`...) são as mesmas chaves do JSON do HX Edit (.hlx/.hxb).
- [ainda não extraído] Pastas e ordem do navegador de modelos (parte 0, listas do browser), ícones (partes 7 e 8).

## O que o HXtra v1.2.1 muda (base da camada pública)

- [fato] Partes alteradas: 0, 2, 3, 4, 7 e 8. Iguais: 1 (bootloader), 5, 6 e 9–14.
- [fato] Modelos novos: `HD2_DistNAMLite` (NAM A2 Lite, preset_id 555, 10 parâmetros) e `HD2_ClickMetronomeMono` (Metronome).
- [fato] 26 modelos com parâmetros novos: FX Loop/Return (mono e stereo) +`NoiseGate`, `GateThreshold`, `GateDecay`; 6 compressores (mono e stereo) +`GRPosition`, `GRDirection`, `GRThickness`; Noise Gate +`InputSidechain`.
- [HXtra] Os snapshots 4–6 vão no arquivo do preset na chave `0x64` (bin16 `"HX6B"`, v1, 3 registros). O `.hlx` e o `.hxb` do HX Edit **não** guardam essa chave.

## Backups .hxb

- [fato] Cabeçalho `AF6L` seguido de streams zlib: globais (JSON `DSP/EQ/System/Tuner`), 2 IRs (WAV), 17 `L6ModelFavorite`, 1 `L6UMDArchive` e 1 `L6Setlist` com 126 presets. Cada preset tem só `snapshot0..2`.

## Capturas do HX Edit (2026-11-21, `work/captures/`, fora do git)

Feitas com USBPcap no hub 5, dispositivo 1, e decodificadas por `scripts/decode_capture.py`. Tudo nesta seção é [fato] observado na captura. Os nomes entre aspas são [hipótese] de significado.

- **Portas:** o HX Edit usa a porta local `0x1080` para o editor (o HXtra usa `0x1000`), `0x1001` para o librarian e `0x1002` para o notify. Antes de tudo, ele abre o serviço 5 na porta `0x3ef` e recebe 24 bytes: `05 "P33Main" … 0x03800000`, a identificação e a versão, e fecha.
- **Ao conectar (`01`):**
  - editor: `0x4c` (bloco de floats, "globais/EQ?"), `0x18 {0x76: id}` várias vezes ("lê setting por id"), `0x17`, `0x16`, `0x63`;
  - librarian: `0xfe`, `0x00` (setlists: `PRESETS`), `0x70` (favoritos), `0x01 {0x6b: setlist, 0x65: 2}` (lista de presets), `0x0d {0x65: 2}` (lista de IRs).
  - Nenhum preset inteiro é baixado.
- **Exportar preset (`02`):** librarian `0xff`, depois **`0x04 {0x6b: setlist, 0x6c: slot, 0x65: 2}`**, depois `0xfe`. O `0x04` devolve o **arquivo do preset** (`l6-helix`, 10 chaves: as 9 do edit buffer, com tamanhos idênticos, mais **`0x64`**, bin16 de 1832 bytes começando com `HX6B` v1). O `.hlx` gerado tem só `snapshot0..2`.
- **Backup completo (`04`):** editor `0x55` (msgpack de 714 bytes, "globais?"). Librarian:
  - `0xff`;
  - `0x0c {0x70: i}` (metadados do IR: nome, crc `0x71`...) e `0x0b {0x70: i, 0x65: 2}` (8192 bytes do IR), uma vez para cada um dos 4 IRs;
  - `0x71 {0x76: i}` 17 vezes (favoritos);
  - `0x6d {0x40: n, 0x6a: bool[, 0x69: x]}` **1162 vezes**, sempre com resposta nil ("user model defaults?");
  - `0x04` para os slots 0..125;
  - `0xfe`.
  - O `.hxb` resultante não tem `HX6B` nem `snapshot3`, embora o preset 10 tenha banco B no aparelho.
- **Os comandos de backup só leem.** Os 126 arquivos crus do backup (com o banco B do preset 10) estão em `work/captures/04-bins/`.

## Backup pelo `hxs` (2026-11-21)

- [fato] `hxs backup` (crates `hxs-core` + `hxs-cli`, só leitura) baixou 126 presets, 4 IRs, 17 favoritos e os globais em cerca de 2 s. Os 126 arquivos de preset, os 4 IRs e o blob de globais são **byte a byte idênticos** aos que o HX Edit recebeu na captura `04`. O preset 010 traz a seção `0x64` (snapshots 4–6).
- [fato] A captura `06-hxs-backup` mostra uma sessão limpa: SYN e select nos dois canais, todas as respostas com status 0, e FIN confirmado pelo aparelho.
- [fato] Com o HX Edit aberto, abrir o dispositivo falha no Windows com o erro de sistema 5 (`ERROR_ACCESS_DENIED`, `nusb` `ErrorKind::Other`); o `hxs` mostra isso como "interface em uso".
- [fato] Um dos nomes de IR vem do aparelho com `?` literais no lugar de alguns caracteres; não é erro de decodificação do `hxs`.
- Não incluído no backup: os 1162 `0x6d` do HX Edit, que sempre respondem nil.

## Snapshots 4–6 decodificados (2026-11-21, preset 010)

- [HXtra] Banco B = `"HX6B"`, versão 1, 3 bytes livres, e 3 registros crus de `0x260` bytes do gerenciador de snapshots.
- [fato, cruzando o registro cru com a seção `0x0a` e o `.hlx` do mesmo preset] Layout do registro:
  - `+0x00..0x13`: 20 estados, um por posição do grid (`preset[0][22]`); **`1` = bloco desligado**. Na seção `0x0a`, cada posição é um par `[?, ligado]`. As 20 posições batem neste preset; o primeiro bool vale sempre `false` aqui, então seu papel é [hipótese];
  - `+0x28`: 64 controllers de 7 bytes `{u8 índice, u8 tipo (3 = float), f32 LE valor, u8 footswitch ligado}`. O índice `0x40` marca entrada sem uso. Equivale a `[fs, índice, valor]` na seção `0x0a`;
  - `+0x1e8`: 11 footswitches de 9 bytes `{0x0d, bool, 7 bytes}`, equivalente ao `[13, false, [0]*7]` da seção `0x0a`;
  - `+0x24b`: válido; `+0x250`: tempo (f32); `+0x255`: nome (11 bytes).
- [fato] Controllers do preset 010 (seção 4 + `.hlx`): 0 = Pedal do Volume Pedal; 1 e 2 = Decay e Mix do Dynamic Plate (footswitch `more VERB`); 3 e 4 = Feedback e Mix do Transistor Tape (footswitch `more DELAY`).
- [fato] O snapshot 4 tem os controllers 1–4 no máximo (2,4 / 0,33 / 0,52 / 0,36), com footswitch ligado; o Chorus (A6) está desligado como nos 1–3. Os snapshots 5 e 6 têm `valid = 0`. Isso confere com o que o usuário configurou ("More Delay e More Reverb ativados").
- [fato] Campos de `+0x248..+0x24f` ainda sem significado conhecido (o HXtra cita `+0x24c` como "nome personalizado"). A cor do LED (`12` na seção `0x0a`) não foi localizada no registro.

## Upload de preset (2026-11-21, capturas `07` e `09`)

- [fato] Importar pelo HX Edit = librarian `0xff`, depois **`0x08 {0x6b: setlist, 0x6c: slot, 0x6d: str "nome\0", 0x7b: false, 0x7c: false, 0x7d: 0, 0x6e: str16(arquivo)}`**, depois `0xfe`. O arquivo vai como **str** de bytes crus, não como bin.
- [fato] O `0x08` responde status 1. A conclusão chega pelo notify como `{0x69: 1, 0x6a: {0x66: seq, 0x67: 0, 0x68: nil}}`. Outros eventos no mesmo momento: `6` (nome do slot), `8` e `4` (`{0x6b, 0x6c}`), `0x27`.
- [fato] Os eventos `8 → 0x27 → 4` também aparecem quando o HX Edit **carrega** um preset (`0x14`). Inferência: `8` e `4` são o início e o fim do carregamento de um preset; eles apareceram no upload porque o slot 011 era o preset atual.
- [fato] O HX Edit, a partir do `.hlx`, sobe um arquivo **reconstruído**: as seções 0, 3, 5 e 10 saem com bytes diferentes do original, e sem a `0x64`.
- [fato] `hxs restore-preset` subiu o arquivo cru do slot 010 (4950 B, com a `0x64`) no slot 011. Uma leitura **0,25 s depois da conclusão** voltou com 270 bytes diferentes a partir do offset 4608 (fronteira de setor de 512 B). Uma leitura posterior veio **idêntica byte a byte**, ou seja, o aparelho grava o arquivo como foi enviado.
- [inferência] A confirmação do `0x08` chega antes de o aparelho terminar de gravar ou de recarregar o slot atual. O HX Edit só volta a ler depois do evento 4. O `hxs` agora espera o evento 4 quando o destino é o preset atual.
- [fato, captura `10`] Com a espera pelo evento 4 (destino = preset atual), a leitura de volta veio idêntica de primeira.
- [fato, captura `11`] Destino que **não** é o preset atual (slot 009): só os eventos `6` (nome) e `1` (conclusão), sem `8`/`4`. A leitura de volta imediata veio idêntica. A corrida só apareceu no recarregamento do preset atual.
- [fato] O build HXtra v1.2.1 do usuário não tem o espelho de tela (`0x52` recusado com status 255, erro −45).
- [verificado no aparelho pelo usuário] Seguindo o procedimento (snapshot 1 aceso, depois o footswitch 1 de novo para ir ao banco B), o snapshot 4 do preset restaurado veio com o "more DELAY" e o "more VERB" ligados. Na primeira tentativa os parâmetros estavam nos mínimos, provavelmente porque a troca partiu de outro footswitch (2 → snapshot 5), o que é [hipótese].

## Escritas de IR, configurações e favoritos (2026-11-21, captura `12`)

Ações do usuário no HX Edit: importar um IR e apagá-lo; mudar FS4 Function e voltar; criar um favorito a partir de um bloco e apagá-lo. Tudo nesta seção é [fato] da captura, salvo quando marcado.

| Ação | Comando | Parâmetros | Resposta |
|---|---|---|---|
| Importar IR | librarian `0x09` | `{0x70: slot, 0x71: u32, 0x6d: nome, 0x72: 1, 0x73: 3, 0x7b: false, 0x7c: false, 0x7d: 0, 0x6e: 8192 B}`, os mesmos campos que o `0x0c` devolve, mais os dados do `0x0b` | status 1, conclusão no notify |
| Apagar IR | librarian `0x0f` | `{0x70: slot}` | |
| Escrever configuração | editor `0x19` | `{0x76: id, 0x77: valor}` | notify `0x16` `{0x76: id, 0x77: valor}` |
| Selecionar bloco | editor `0x4e` | `{0x62: bloco, 0x1a: 0}` | notify `0x27` `{0x62, 0x1a}` |
| Pôr um modelo num bloco | editor `0x27` | `{0x62: bloco, 0x63: {0x13: 6, 0x14: corpo do bloco}}` | notify `0x31` `{0x62}` |
| Salvar bloco como favorito | editor `0x77` | `{0x62: bloco, 0x76: n°, 0x1f: true, 0x6d: nome}` | notify `0x38` |
| Remover bloco | editor `0x2d` | `{0x62: bloco}` | |
| Apagar favorito | librarian `0x74` | `{0x76: n°}` | notify `0x3b` `{0x76}` |
| Carregar preset | editor `0x14` | `{0x6b, 0x6c}` | status 1, conclusão no notify (tipo `0x14`), depois `8 → 0x27 → 4` |

- O notify `0x16`, que chega o tempo todo, é "configuração mudou" `{0x76: id, 0x77: valor}`.
- IDs: 97, 98 e 99 são FS3, FS4 e FS5 Function (tabela de páginas do HXtra em `m4/settings.md`). O FS4 foi `1 → 2 → 1`; o FS5 não foi escrito.
- Backup depois das ações, comparado com o das 14:01 (conferência só com leitura):
  - os 126 presets e os 4 IRs são idênticos; o IR importado e o bloco posto no preset 011 não ficaram;
  - nos globais, só o ID 28 mudou (10 → 1), e o valor bate com o preset atual (001); [inferência] o ID 28 é o `LastActivePreset` do `.hxb`;
  - nos 17 favoritos, só um campo mudou: a chave 28 dentro de `0x40` caiu 1 em todos (1..17 → 0..16). Criar e apagar um favorito pelo HX Edit renumera esse campo. Os parâmetros não mudaram.
- [em aberto] O `0x77` cria um favorito a partir de um bloco do edit buffer. Ainda não conheço um comando que grave um favorito a partir de dados (para a restauração), nem uma escrita em bloco dos globais.

## Em aberto

- ~~Qual comando entrega o arquivo do preset~~ → resolvido: librarian `0x04` (acima). [fato] O `0x16` (edit buffer) **não** traz a `0x64`, nem do preset 10, que tem banco B.
- Formato dos 3 registros de snapshot dentro da `0x64` (610 bytes cada), para mostrar e editar os snapshots 4–6.
- Comandos de escrita: subir um preset para um slot (e se ele preserva a `0x64`), salvar, renomear, restaurar o backup, IRs. Exigem captura de ações que gravam no aparelho.

## Resolvido

- [fato] A lista do librarian `0x01` vem **na ordem dos slots**. Cada item é `{id_do_arquivo: {0x6d: nome, 0x7b, 0x7c, 0x7d}}`, e a chave é o ID do arquivo no armazenamento, não o slot. O `hx_ctl.py preset_names` ordena pela chave e por isso embaralha a lista. Exemplo: slot 0 "AC30 Clean" = arquivo 2; slot 125 "No Amp" = arquivo 0. Isso bate com a nota do HXtra (`dir[perm[slot]].id`; mover um preset permuta `perm`).
- [fato] A troca de preset durante as leituras foi o usuário mexendo no aparelho, não um efeito dos comandos.
