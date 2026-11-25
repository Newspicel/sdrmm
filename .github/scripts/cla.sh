#!/usr/bin/env bash
set -euo pipefail

readonly branch=cla-signatures
readonly path=signatures/cla.json
readonly phrase='I have read the SDRmm CLA and I hereby sign it.'
readonly marker='<!-- cla -->'
readonly bot='github-actions[bot]'
readonly event="$GITHUB_EVENT_PATH"
readonly repo="$GITHUB_REPOSITORY"

owner=$(jq -r .repository.owner.login "$event")
pr=$(jq -r '.pull_request.number // .issue.number' "$event")
version=$(sed -n 's/^Version //p' CLA.md)
cla_url="$GITHUB_SERVER_URL/$repo/blob/$GITHUB_SHA/CLA.md"
head_sha=''
authors=''
signatures=''
signatures_sha=''

fail() {
  echo "::error::$1"
  exit 1
}

set_status() {
  gh api -X POST "repos/$repo/statuses/$head_sha" -f state="$1" -f context=CLA \
    -f description="${2:0:140}" -f target_url="$cla_url" >/dev/null
}

load_pull() {
  local info
  info=$(gh api "repos/$repo/pulls/$pr")
  head_sha=$(jq -r .head.sha <<<"$info")
  if (($(jq .commits <<<"$info") > 250)); then
    set_status failure "Over 250 commits, a maintainer checks the CLA by hand"
    fail "GitHub lists at most 250 commits of a pull request"
  fi
  authors=$(gh api --paginate --slurp "repos/$repo/pulls/$pr/commits?per_page=100" | jq -c '
    add
    | map({id: .author.id, login: .author.login, type: .author.type, email: .commit.author.email})
    | unique_by(.id // .email)')
}

load_signatures() {
  local out err
  err=$(mktemp)
  if out=$(gh api "repos/$repo/contents/$path?ref=$branch" 2>"$err"); then
    signatures_sha=$(jq -r .sha <<<"$out")
    signatures=$(jq -r '.content | @base64d' <<<"$out")
  elif grep -q 'HTTP 404' "$err"; then
    signatures_sha=''
    signatures='[]'
  else
    cat "$err" >&2
    fail "Could not read $path on $branch"
  fi
}

unsigned() {
  jq -c --argjson sigs "$signatures" --arg v "$version" --arg owner "$owner" '
    map(select(.type != "Bot" and .login != $owner)
      | . as $a
      | select($a.id == null or (any($sigs[]; .id == $a.id and .version == $v) | not)))' <<<"$authors"
}

comment_signs() {
  jq -e --arg p "$phrase" '(.comment.body // "") | gsub("^\\s+|\\s+$"; "") == $p' "$event" >/dev/null
}

signer_unsigned() {
  jq -e --argjson id "$(jq .comment.user.id "$event")" 'any(.[]; .id == $id)' <<<"$(unsigned)" >/dev/null
}

with_signature() {
  jq --argjson c "$(jq -c .comment "$event")" --arg v "$version" --argjson pr "$pr" '
    . + [{id: $c.user.id, login: $c.user.login, version: $v, signed_at: $c.created_at,
          pull_request: $pr, comment: $c.html_url}]' <<<"$signatures"
}

put_file() {
  jq -n --arg m "$2" --arg b "$branch" --arg c "$1" --arg sha "$signatures_sha" '
    {message: $m, branch: $b, content: ($c | @base64)} + (if $sha == "" then {} else {sha: $sha} end)' |
    gh api -X PUT "repos/$repo/contents/$path" --input - >/dev/null
}

create_branch() {
  local tree commit
  tree=$(jq -n --arg p "$path" --arg c "$1" '{tree: [{path: $p, mode: "100644", type: "blob", content: $c}]}' |
    gh api -X POST "repos/$repo/git/trees" --input - --jq .sha)
  commit=$(jq -n --arg m "$2" --arg t "$tree" '{message: $m, tree: $t, parents: []}' |
    gh api -X POST "repos/$repo/git/commits" --input - --jq .sha)
  gh api -X POST "repos/$repo/git/refs" -f ref="refs/heads/$branch" -f sha="$commit" >/dev/null
}

store_signatures() {
  if [[ -n $signatures_sha ]] || gh api "repos/$repo/branches/$branch" >/dev/null 2>&1; then
    put_file "$1" "$2"
  else
    create_branch "$1" "$2"
  fi
}

sign() {
  local attempt message
  message="CLA: $(jq -r .comment.user.login "$event") signed in #$pr"
  for attempt in 1 2 3; do
    load_signatures
    signer_unsigned || return 0
    if store_signatures "$(with_signature)" "$message"; then
      load_signatures
      return 0
    fi
    echo "::warning::Storing the signature failed on attempt $attempt"
  done
  fail "Could not store the signature"
}

bot_comment_id() {
  gh api --paginate --slurp "repos/$repo/issues/$pr/comments?per_page=100" |
    jq -r --arg bot "$bot" --arg m "$marker" '
      add | map(select(.user.login == $bot and (.body | startswith($m)))) | first | .id // empty'
}

write_comment() {
  local id
  id=$(bot_comment_id)
  if [[ -n $id ]]; then
    gh api -X PATCH "repos/$repo/issues/comments/$id" -f body="$1" >/dev/null
  elif [[ $2 == ask ]]; then
    gh api -X POST "repos/$repo/issues/$pr/comments" -f body="$1" >/dev/null
  fi
}

ask_body() {
  printf '%s\nThanks! Before we can merge, please sign the [CLA](%s) by commenting:\n\n~~~\n%s\n~~~\n' \
    "$marker" "$cla_url" "$phrase"
  [[ -z $1 ]] || printf '\nNot signed yet: %s\n' "$1"
  [[ -z $2 ]] || printf '\nNo GitHub account for %s. Add the email to your account, then comment recheck.\n' "$2"
}

report() {
  local logins unlinked
  if [[ $(jq length <<<"$1") == 0 ]]; then
    set_status success "All contributors signed"
    write_comment "$(printf '%s\nAll contributors signed the [CLA](%s).' "$marker" "$cla_url")" resolve
    return
  fi
  logins=$(jq -r 'map(select(.id) | "@" + .login) | join(", ")' <<<"$1")
  unlinked=$(jq -r 'map(select(.id == null) | .email) | join(", ")' <<<"$1")
  set_status failure "Waiting for: $(jq -r 'map(.login // .email) | join(", ")' <<<"$1")"
  write_comment "$(ask_body "$logins" "$unlinked")" ask
}

main() {
  [[ -n $version ]] || fail "CLA.md has no Version line"
  load_pull
  load_signatures
  if comment_signs; then
    sign
  fi
  report "$(unsigned)"
}

main
