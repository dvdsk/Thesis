text_sources = $(addpostfix content $(addpostfix .tex main.tex))
figs = $(addprefix figs/papers/, raft_states.svg)

latex_temps = main.aux main.bbl main.blg main.log main.toc main.out \
main.fls main.xdv missfont.log main.fdb_latexmk main.run.xml main.bcf "main.synctex(busy)"

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

