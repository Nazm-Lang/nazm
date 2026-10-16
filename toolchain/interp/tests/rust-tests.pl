#!/usr/bin/perl
# The programs the Rust CLI tests embed, for `run.sh --rust-tests` (openspec/changes/r4-interpreter,
# design.md, *The Rust tests' programs*):
#
#     perl rust-tests.pl OUT_DIR crates/nazm-cli/tests/*.rs
#
# writes each program to OUT_DIR and prints the path of each one to run, one per line.
#
# - A string literal with a `fn main` in it — raw, or plain with Rust's escapes and line
#   continuations undone — is a program of one file, `BASE-N.nz`. A format string (one with `{{`)
#   is left out: its text is not the program. Character literals are stepped over, so a `'"'`
#   starts nothing.
# - A literal the test names as a file — `("lib.nz", "...")`, `write(dir, "lib.nz", "...")`,
#   `.join("lib.nz"), r#"..."#`, or the name beside a `const` holding the text — belongs to the
#   files of its test (from one `#[test]` to the next), written in order as the test's directory
#   holds them: a name written again first closes the program so far. A program of two files or
#   more is written to `BASE-mN/`, its root the file with a `fn main` written last. That root's
#   literal is not also a program of one file; the others still are, as before, whether or not
#   they find their modules alone.
use strict;
use warnings;
use File::Path qw(make_path);

my $out = shift @ARGV;
make_path($out);

sub plain {
    my $s = shift;
    $s =~ s/\\\n\s*//g;
    $s =~ s/\\(u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)/
        my $e = $1;
        $e eq "n" ? "\n" : $e eq "t" ? "\t" : $e eq "r" ? "\r" : $e eq "0" ? "\0" :
        $e =~ m{^u\{(.*)\}$} ? chr(hex($1)) : $e =~ m{^x(..)$} ? chr(hex($1)) : $e/ge;
    return $s;
}

sub put {
    my ($path, $text) = @_;
    (my $dir = $path) =~ s{/[^/]*$}{};
    make_path($dir);
    open(my $f, ">", $path) or die "$path: $!";
    print $f $text;
    close($f);
}

for my $t (@ARGV) {
    local $/;
    open(my $fh, "<", $t) or die "$t: $!";
    my $src = <$fh>;
    close($fh);
    (my $base = $t) =~ s{.*/}{};
    $base =~ s{\.rs$}{};

    # The texts `const NAME: &str = ...;` holds.
    my %consts;
    while ($src =~ /\bconst\s+([A-Z_][A-Z0-9_]*)\s*:\s*&str\s*=\s*(?:r(#*)"(.*?)"\2|"((?:[^"\\]|\\.)*)")\s*;/sg) {
        $consts{$1} = defined $3 ? $3 : plain($4);
    }
    # Where each test starts: a named file belongs to the last one before it. (A line starting
    # `fn` may be inside a program's text; `#[test]` is not.)
    my @fns;
    while ($src =~ /^\s*#\[test\]/mg) { push @fns, $-[0]; }
    my $fn_of = sub {
        my $at = shift;
        my $k = -1;
        for my $i (0 .. $#fns) { last if $fns[$i] > $at; $k = $i; }
        return $k;
    };
    # Named files, by function: [name, text, where the literal text starts].
    my (%files, %named_at);
    while ($src =~ /"([\w.\/-]+\.nz)"\s*\)?\s*,\s*&?\s*(?:(r(#*)")(.*?)"\3|(")((?:[^"\\]|\\.)*)"|([A-Z_][A-Z0-9_]*)\b)/sg) {
        my ($name, $text, $lit) = ($1, undef, undef);
        if (defined $2) { $text = $4; $lit = $-[2]; }
        elsif (defined $5) { $text = plain($6); $lit = $-[5]; }
        elsif (exists $consts{$7}) { $text = $consts{$7}; }
        # A name beside a word that is not Nazm (`"main.nz", "-o"`) names no file's text.
        next unless defined $text && $text =~ /\b(?:fn|use|pub|struct|enum|const|impl|trait|static)\b/;
        push @{ $files{ $fn_of->($-[0]) } }, [$name, $text, $lit];
    }
    # Each test's files as its directory holds them: a name written again starts the next program,
    # which keeps the files written before it (a test that edits one module and checks again).
    my $m = 0;
    my %seen;
    my $emit = sub {
        my ($cur, $order) = @_;
        return if @$order < 2;
        # The root: the file with a `fn main` written last.
        my ($root) = grep { $cur->{$_}[0] =~ /fn main\b/ } reverse @$order;
        return unless defined $root;
        my $key = join("\0", map { "$_\0$cur->{$_}[0]" } sort @$order) . "\0$root";
        return if $seen{$key}++;
        $m++;
        my $dir = "$out/$base-m$m";
        put("$dir/$_", $cur->{$_}[0]) for @$order;
        print "$dir/$root\n";
        $named_at{ $cur->{$root}[1] } = 1 if defined $cur->{$root}[1];
    };
    for my $k (sort { $a <=> $b } keys %files) {
        my (%cur, @order);
        for my $f (@{ $files{$k} }) {
            my ($name, $text, $lit) = @$f;
            if (exists $cur{$name}) {
                $emit->(\%cur, \@order);
            } else {
                push @order, $name;
            }
            $cur{$name} = [$text, $lit];
        }
        $emit->(\%cur, \@order);
    }

    # Programs of one file, numbered as before the files above were taken out.
    my $n = 0;
    while ($src =~ /\br(#*)"(.*?)"\1|(?<![\w])\x27(?:[^\x27\\\n]|\\.[^\x27\n]*)\x27|"((?:[^"\\]|\\.)*)"/sg) {
        my $at = $-[0];
        my $text;
        if (defined $2) {
            $text = $2;
        } elsif (defined $3) {
            $text = $3;
            next if $text =~ /\{\{|\}\}/;
            $text = plain($text);
        } else {
            next;
        }
        next unless $text =~ /fn main\b/;
        $n++;
        next if $named_at{$at};
        put("$out/$base-$n.nz", $text);
        print "$out/$base-$n.nz\n";
    }
}
