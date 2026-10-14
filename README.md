# Ferramenta de Captura do W11 para Pop!_OS COSMIC

Clone da **Ferramenta de Captura** (Snipping Tool) do W11 para o **Pop!_OS
COSMIC**, feito em Rust direto sobre os protocolos Wayland do COSMIC. O programa
se chama `recorte`.

## Instalar

**Pronto para usar** (Pop!_OS 24.04 COSMIC, x86_64): baixe o pacote da
[última release](https://github.com/Filipeparreirab/Ferramenta-de-Captura-do-W11-para-Pop_OS-COSMIC/releases/latest) e rode:

```bash
tar xzf recorte-*-x86_64.tar.gz && cd recorte-*-x86_64 && ./install.sh
```

**A partir do código** (requer Rust, `libgtk-4-dev` e `libadwaita-1-dev`):

```bash
git clone https://github.com/Filipeparreirab/Ferramenta-de-Captura-do-W11-para-Pop_OS-COSMIC recorte
cd recorte
./install.sh               # compila, instala em ~/.local e liga Print / Super+Shift+S
./install.sh --sem-atalhos # instala sem mexer nos atalhos
```

O instalador grava o binário em `~/.local/bin/recorte`, o atalho no menu de
aplicativos ("Ferramenta de Captura", com a ação "Novo recorte") e os atalhos de
teclado do COSMIC. O arquivo de atalhos anterior fica salvo como
`~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom.bak-DATA`.

Para criar o atalho manualmente: **Configurações › Teclado › Atalhos
personalizados**, comando `/home/SEU_USUARIO/.local/bin/recorte`.

Requer o COSMIC (usa `ext-image-copy-capture`, `wlr-layer-shell`,
`zcosmic-toplevel-info` e `ext-data-control`).

## Uso

| Ação | Como |
|---|---|
| Novo recorte | `Print` ou `Super + Shift + S` |
| Modos | Barra flutuante: retângulo, forma livre, janela, tela inteira |
| Cancelar | `Esc` (durante o arraste, `Esc` ou botão direito desfaz a seleção) |
| Colar | `Ctrl + V` em qualquer aplicativo — a imagem já está copiada |
| Editar | Clique na notificação "Recorte copiado para a área de transferência" |

A tela é congelada e escurecida assim que o recorte começa; a área selecionada
fica clara. O último modo escolhido na barra é lembrado. Com vários monitores,
a seleção pode atravessar telas com escalas diferentes (a imagem usa a maior).

### Editor

`recorte editar [ARQUIVO]` (ou o ícone no menu). Ferramentas no centro da barra
de título, como no W11:

- Caneta esferográfica e marca-texto (clique de novo na ferramenta para escolher
  cor e espessura); borracha que apaga traços inteiros e "Apagar toda a tinta"
- Formas: retângulo, elipse, linha e seta (`Shift` alinha/quadra; opção de preencher)
- Texto (`Enter` confirma, `Shift + Enter` quebra a linha)
- Recortar imagem com alças (`Enter` aplica, `Esc` cancela)
- `Ctrl+Z` / `Ctrl+Y` desfazer e refazer, `Ctrl + roda do mouse` / `Ctrl +/−/0` zoom
- `Ctrl+S` salvar como PNG, `Ctrl+C` copiar, `Ctrl+N` novo recorte (com modo e
  atraso de 3, 5 ou 10 s), `Ctrl+O` abrir, arrastar uma imagem para a janela

Cada edição é copiada automaticamente para a área de transferência.

### Configurações (menu ⋮ › Configurações)

Salvar automaticamente em PNG (padrão: `~/Imagens/Capturas de tela`), escurecer
a tela, notificação, abrir o editor após cada recorte, borda em cada captura,
perguntar antes de descartar edições e copiar alterações automaticamente.
Ficam em `~/.config/recorte/recorte.conf`.

## Como funciona

| Módulo | Papel |
|---|---|
| `snip.rs`, `wlimpl.rs` | Congela cada monitor (`ext-image-copy-capture`), desenha a sobreposição em superfícies `layer-shell` com software (tiny-skia), redesenha só as regiões alteradas |
| `snip.rs` (janelas) | Geometria e área de trabalho via `zcosmic-toplevel-info`; como o Wayland não informa a ordem de empilhamento, cada janela visível é capturada e comparada com a tela congelada para saber qual está por cima |
| `clip.rs` | No Wayland a área de transferência vive no processo que a oferece: cada cópia inicia `recorte __clip`, que serve o PNG via `ext-data-control` até outro app copiar algo |
| `notify.rs` | Notificação com ação e token de ativação (abre o editor com foco) |
| `editor.rs` | Editor GTK4/libadwaita — herda o tema do COSMIC |
| `shortcuts.rs` | Mescla os atalhos no arquivo do COSMIC sem tocar nos demais |

Nada fica rodando em segundo plano: cada recorte é um processo curto. Medido
neste computador (1920×1080): tela inteira até o PNG salvo em 0,11 s com
0,08 s de CPU e 46 MB de pico; sobreposição aberta usa ~54 MB e praticamente
nenhuma CPU parada.

## Testes

```bash
cargo test --release
```

## Diferenças conhecidas em relação ao Windows

- Sem gravação de vídeo nem ações de texto (OCR).
- Sem compartilhamento ("Compartilhar") e sem impressão.
- O destaque do modo janela aparece quando o mouse se move (o COSMIC só informa
  a posição do ponteiro após um movimento).
