<?php

namespace App\Services;

use App\Models\User;

class UserService
{
    public function load(): User
    {
        return new User();
    }
}
