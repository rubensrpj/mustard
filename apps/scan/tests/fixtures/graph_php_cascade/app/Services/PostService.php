<?php

namespace App\Services;

use App\Models\Comment;
use App\Models\Post;

class PostService
{
    public function load(): Post
    {
        return new Post();
    }

    public function firstComment(): Comment
    {
        return new Comment();
    }
}
