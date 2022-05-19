#!/usr/bin/env bash

set -e

pandoc --from latex+raw_tex --lua-filter=tikz-to-png.lua --wrap=none -s main.tex -o readme.md

# remove lines till Design section
sed -i '1,/Design/d' readme.md

# remove footnotes
sed -i '/[^1]: One /,+99d' readme.md

shorter=`tail -n +3 readme.md` 
echo "$shorter" > readme.md

function replace {
	local keyword=$1
	local replace=$2

	escaped_replace=$(printf '%s\n' "$replace" | sed -e 's/[\/&]/\\&/g')
	escaped_keyword=$(printf '%s\n' "$keyword" | sed -e 's/[]\/$*.^[]/\\&/g');
	sed -i "s/$escaped_keyword/$escaped_replace/g" readme.md
}

# cleanup acronyms
replace '[api]{acronym-label="api" acronym-form="singular+short"}' API
replace '[posix]{acronym-label="posix" acronym-form="singular+short"}' POSIX
replace '[hpc]{acronym-label="hpc" acronym-form="singular+short"}' HPC
replace '[praft]{acronym-label="praft" acronym-form="singular+short"}' pRaft

# cleanup sections references
sed -i -r 's/\[\\\[sec:.+}//g' readme.md
# cleanup labels
sed -i -r 's/\{#.+\}//g' readme.md

# perpend notice
NOTICE="Below is an excerpt of the design section of my thesis automatically converted to markdown. I recommand you instead go to the Design section in my [thesis](report/main.pdf)

"
text=`cat readme.md`

echo "$NOTICE" > readme.md
echo "$text" >> readme.md
