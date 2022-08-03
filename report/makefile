text_sources = $(addpostfix content $(addpostfix .tex main.tex))
figs = $(addprefix figs/papers/, raft_states.svg)
figs += $(addprefix figs/plots/, placeholder.png)
# plots_raw := $(shell find ../results/plots/ -name '*.svg')
# plots_name := $(pathsubst .svg%,%,$(plots_raw)

latex_temps = main.aux main.bbl main.blg main.log main.toc main.out \
main.fls main.xdv missfont.log main.fdb_latexmk main.run.xml main.bcf "main.synctex(busy)"

# figs/plots/%.pdf: ../results/plots/%.svg
# 	@echo "test $<"
# 	@echo "test $@"
# 	mkdir -p figs/plots
# 	inkscape -D $< -o $@ --export-latex

pdf:  ${content} ${figs} References.bib
	-rm ${latex_temps}
	-pdflatex --shell-escape main \
	&& bibtex main; \
	   biber main \
	&& pdflatex main

open: pdf
	xdg-open report/main.pdf

.PHONY : clean
clean :
	-rm ${latex_temps};
	-rm main.pdf

