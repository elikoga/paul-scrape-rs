#!/usr/bin/env bash

# grep "starting" in out.txt, save how many lines in a variable
# grep "finished" in out.txt, save how many lines in a variable

cp out.txt out.txt.bak
starting_lines=$(grep "starting" out.txt.bak | wc -l)
finished_lines=$(grep "fininini" out.txt.bak | wc -l)

# show the difference
echo "starting lines: $starting_lines"
echo "finished lines: $finished_lines"
echo "difference: $(($starting_lines-$finished_lines))"


# do the same with "asdasd_start" and "asdasd_end"

starting_lines=$(grep "asdasd_start" out.txt.bak | wc -l)
finished_lines=$(grep "asdasd_end" out.txt.bak | wc -l)

echo "starting lines: $starting_lines"
echo "finished lines: $finished_lines"
echo "difference: $(($starting_lines-$finished_lines))"
