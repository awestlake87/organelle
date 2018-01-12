cargo publish --token $CARGO_TOKEN || exit 1

VERSION=`cargo pkgid | sed -E 's/.*#(.*:)?(.+)/\2/'`
git tag v$VERSION || exit 2
git push https://awestlake87:$GH_TOKEN@github.com/awestlake87/organelle v$VERSION || exit 3
