from binarytrees import make_tree, check_tree, make_check, get_argchunks

def test_make_tree():
    t = make_tree(2)
    assert t is not None

def test_check_tree():
    t = make_tree(2)
    assert check_tree(t) == 7

def test_make_check():
    assert make_check((1, 2)) == 7

def test_get_argchunks():
    chunks = list(get_argchunks(5, 2, chunksize=2))
    assert len(chunks) == 3
